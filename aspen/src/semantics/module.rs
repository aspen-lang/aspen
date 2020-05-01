use crate::semantics::*;
use crate::syntax::{Navigator, Node, Parser};
use crate::{Diagnostics, Source, URI};
use std::convert::TryInto;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::Mutex;

pub struct Module {
    source: Arc<Source>,
    root_node: Arc<Node>,
    diagnostics: Mutex<Diagnostics>,
    host: Host,

    // Analyzers
    exported_declarations: Memo<&'static ExportedDeclarations>,
    collect_diagnostics: Once<&'static CheckForDuplicateExports>,
}

impl Module {
    pub async fn parse(source: Arc<Source>, host: Host) -> Module {
        let (root_node, diagnostics) = Parser::new(source.clone()).parse_module().await;

        Module {
            source,
            root_node,
            diagnostics: Mutex::new(diagnostics),
            host,

            exported_declarations: Memo::of(&ExportedDeclarations),
            collect_diagnostics: Once::of(&CheckForDuplicateExports),
        }
    }

    pub fn object_file_path(&self) -> Option<PathBuf> {
        let mut path: PathBuf = self.uri().try_into().ok()?;
        path.set_extension("o");
        Some(path)
    }

    pub fn header_file_path(&self) -> Option<PathBuf> {
        let mut path: PathBuf = self.uri().try_into().ok()?;
        path.set_extension("ah");
        Some(path)
    }

    pub fn uri(&self) -> &URI {
        self.source.uri()
    }

    pub fn modified(&self) -> &SystemTime {
        &self.source.modified
    }

    async fn run_analyzer<A: Analyzer>(&self, analyzer: A) -> A::Output {
        let ctx = AnalysisContext {
            uri: self.source.uri().clone(),
            host: self.host.clone(),
            navigator: Navigator::new(self.root_node.clone()),
        };

        analyzer.analyze(ctx).await
    }

    pub async fn diagnostics(&self) -> Diagnostics {
        let d = self.run_analyzer(&self.collect_diagnostics).await;

        let mut diagnostics = self.diagnostics.lock().await;

        if !d.is_empty() {
            diagnostics.push_all(d);
        }

        diagnostics.clone()
    }

    pub async fn exported_declarations(&self) -> Vec<(String, Arc<Node>)> {
        self.run_analyzer(&self.exported_declarations).await
    }
}

impl fmt::Debug for Module {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&self.source.uri(), f)?;
        write!(f, " ")?;
        fmt::Debug::fmt(&self.root_node, f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[tokio::test]
    async fn empty_source() {
        let host = Host::new();
        host.set(Source::new("test:x", "object X.")).await;
        let module = host.get(&"test:x".into()).await.unwrap();

        let exported: HashMap<_, _> = module.exported_declarations().await.into_iter().collect();
        assert_eq!(exported.len(), 1);
        assert!(exported.get("X").is_some());
    }

    #[tokio::test]
    async fn duplcated_export() {
        let host = Host::new();
        host.set(Source::new("test:x", "object X. class X.")).await;
        let module = host.get(&"test:x".into()).await.unwrap();

        let diagnostics = module.diagnostics().await;
        assert_eq!(diagnostics.len(), 1);
    }
}
