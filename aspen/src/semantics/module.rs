use crate::semantics::*;
use crate::syntax::{Navigator, Node, Parser};
use crate::{Diagnostics, Source, URI};
use std::sync::Arc;
use tokio::sync::{Mutex, MutexGuard};
use futures::future;

pub struct Module {
    uri: URI,
    root_node: Arc<Node>,
    diagnostics: Mutex<Diagnostics>,

    #[allow(unused)]
    host: Host,

    // Analyzers
    exported_declarations: Memo<ExportedDeclarations, Vec<(String, Arc<Node>)>>,
    check_for_duplicated_exports: Diagnose<CheckForDuplicateExports>,
}

impl Module {
    pub async fn parse(source: Arc<Source>, host: Host) -> Module {
        let uri = source.uri().clone();
        let (root_node, diagnostics) = Parser::new(source).parse_module().await;

        Module {
            uri,
            root_node,
            diagnostics: Mutex::new(diagnostics),
            host,

            exported_declarations: Memo::of(ExportedDeclarations),
            check_for_duplicated_exports: Diagnose::with(CheckForDuplicateExports),
        }
    }

    async fn run_analyzer<'a, A: Analyzer<'a, T>, T>(&'a self, analyzer: A) -> T {
        let ctx = AnalysisContext {
            uri: self.uri.clone(),
            host: self.host.clone(),
            navigator: Navigator::new(self.root_node.clone()),
        };

        analyzer.analyze(ctx).await
    }

    pub async fn diagnostics<'a, 'b: 'a>(&'b self) -> MutexGuard<'a, Diagnostics> {
        println!("DIAGNOSTICS");
        let (d1, _) = future::join(
            self.run_analyzer(&self.check_for_duplicated_exports),
            self.exported_declarations(),
        ).await;


        let mut diagnostics = self.diagnostics.lock().await;

        if !d1.is_empty() {
            diagnostics.push_all(d1);
        }

        diagnostics
    }

    pub async fn exported_declarations(&self) -> Vec<(String, Arc<Node>)> {
        self.run_analyzer(&self.exported_declarations).await
    }

    pub async fn check_for_duplicated_exports(&self) {
        let d = self.run_analyzer(&self.check_for_duplicated_exports).await;

        if !d.is_empty() {
            self.diagnostics.lock().await.push_all(d);
        }
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
