use crate::semantics::*;
use crate::syntax::{Declaration, Navigator, Parser, Root};
use crate::{Diagnostics, Source, SourceKind, URI};
use std::fmt;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::Mutex;

pub struct Module {
    source: Arc<Source>,
    root_node: Arc<Root>,
    diagnostics: Mutex<Diagnostics>,
    pub host: Host,

    // Analyzers
    exported_declarations: Memo<&'static analyzers::GetExportedDeclarations>,
    collect_diagnostics: Once<&'static analyzers::CheckForDuplicateExports>,
}

impl Module {
    pub async fn parse(source: Arc<Source>, host: Host) -> Module {
        let (root_node, diagnostics) = Parser::new(source.clone()).parse().await;

        Module {
            source,
            root_node,
            diagnostics: Mutex::new(diagnostics),
            host,

            exported_declarations: Memo::of(&analyzers::GetExportedDeclarations),
            collect_diagnostics: Once::of(&analyzers::CheckForDuplicateExports),
        }
    }

    pub fn uri(&self) -> &URI {
        self.source.uri()
    }

    pub fn kind(&self) -> &SourceKind {
        &self.source.kind
    }

    pub fn syntax_tree(&self) -> &Arc<Root> {
        &self.root_node
    }

    pub fn modified(&self) -> &SystemTime {
        &self.source.modified
    }

    pub fn navigate(&self) -> Arc<Navigator> {
        Navigator::new(self.root_node.clone())
    }

    async fn run_analyzer<A: Analyzer>(&self, analyzer: A, input: A::Input) -> A::Output {
        let ctx = AnalysisContext {
            input,
            uri: self.source.uri().clone(),
            host: self.host.clone(),
            navigator: Navigator::new(self.root_node.clone()),
        };

        analyzer.analyze(ctx).await
    }

    pub async fn diagnostics(&self) -> Diagnostics {
        let d = self.run_analyzer(&self.collect_diagnostics, ()).await;

        let mut diagnostics = self.diagnostics.lock().await;

        if !d.is_empty() {
            diagnostics.push_all(d);
        }

        diagnostics.clone()
    }

    pub async fn exported_declarations(&self) -> Vec<(String, Arc<Declaration>)> {
        self.run_analyzer(&self.exported_declarations, ()).await
    }
}

impl fmt::Debug for Module {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?} {:?}", self.source.uri(), self.root_node)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Context;
    use std::collections::HashMap;

    #[tokio::test]
    async fn single_declaration() {
        let host = Host::new(Arc::new(Context::test()));
        host.set(Source::new("test:x", "object X.")).await;
        let module = host.get(&"test:x".into()).await.unwrap();

        let exported: HashMap<_, _> = module.exported_declarations().await.into_iter().collect();
        assert_eq!(exported.len(), 1);
        assert!(exported.get("X").is_some());
    }

    #[tokio::test]
    async fn duplicated_export() {
        let host = Host::new(Arc::new(Context::test()));
        host.set(Source::new("test:x", "object X. class X.")).await;
        let module = host.get(&"test:x".into()).await.unwrap();

        let diagnostics = module.diagnostics().await;
        assert_eq!(diagnostics.len(), 1);
    }
}
