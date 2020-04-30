use crate::semantics::{Host, Module};
use crate::syntax::{Navigator, Node, NodeKind};
use crate::{Diagnostics, DuplicateExport, URI};
use std::borrow::BorrowMut;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct AnalysisContext<'a> {
    pub uri: URI,
    pub host: Host,
    pub navigator: Navigator<'a>,
}

impl<'a> AnalysisContext<'a> {
    pub async fn current_module(&self) -> Arc<Module> {
        self.host.get(&self.uri).await.unwrap()
    }
}

#[async_trait]
pub trait Analyzer<'a, T> {
    async fn analyze(self, ctx: AnalysisContext<'a>) -> T;
}

pub struct Memo<A, T> {
    mutex: Mutex<Option<T>>,
    analyzer: A,
}

impl<A, T> Memo<A, T> {
    pub fn of(analyzer: A) -> Memo<A, T> {
        Memo {
            mutex: Mutex::new(None),
            analyzer,
        }
    }
}

#[async_trait]
impl<'a, A, T> Analyzer<'a, T> for &'a Memo<A, T>
where
    A: Analyzer<'a, T> + Clone + Sync + Send,
    T: Clone + Send,
{
    async fn analyze(self, ctx: AnalysisContext<'a>) -> T {
        let mut opt = self.mutex.lock().await;

        match opt.as_ref() {
            Some(t) => return t.clone(),
            None => {}
        }
        let analyzer = self.analyzer.clone();
        let t = analyzer.analyze(ctx).await;
        *opt = Some(t.clone());
        t
    }
}

pub struct Diagnose<A> {
    analyzer: Mutex<Option<A>>,
}

impl<A> Diagnose<A> {
    pub fn with(analyzer: A) -> Diagnose<A> {
        Diagnose {
            analyzer: Mutex::new(Some(analyzer)),
        }
    }
}

#[async_trait]
impl<'a, A> Analyzer<'a, Diagnostics> for &'a Diagnose<A>
where
    A: Analyzer<'a, Diagnostics> + Send,
{
    async fn analyze(self, ctx: AnalysisContext<'a>) -> Diagnostics {
        let mut lock = self.analyzer.lock().await;
        let opt: &mut Option<A> = lock.borrow_mut();
        if opt.is_none() {
            return Diagnostics::new();
        }
        let analyzer = std::mem::replace(opt, None).unwrap();
        analyzer.analyze(ctx).await
    }
}

#[derive(Clone)]
pub struct ExportedDeclarations;

#[async_trait]
impl<'a> Analyzer<'a, Vec<(String, Arc<Node>)>> for ExportedDeclarations {
    async fn analyze(self, ctx: AnalysisContext<'a>) -> Vec<(String, Arc<Node>)> {
        println!("EXPORTED DECS");
        tokio::time::delay_for(tokio::time::Duration::new(5, 0)).await;

        let mut exported_declarations = vec![];
        for declaration in ctx.navigator.children() {
            match &declaration.node.kind {
                NodeKind::ObjectDeclaration { symbol, .. }
                | NodeKind::ClassDeclaration { symbol, .. } => {
                    if let Some(symbol) = symbol.as_ref().map(Arc::as_ref).and_then(Node::symbol) {
                        exported_declarations.push((symbol, declaration.node.clone()));
                    }
                }
                _ => {}
            }
        }
        exported_declarations
    }
}

#[derive(Clone)]
pub struct CheckForDuplicateExports;

#[async_trait]
impl<'a> Analyzer<'a, Diagnostics> for CheckForDuplicateExports {
    async fn analyze(self, ctx: AnalysisContext<'a>) -> Diagnostics {
        println!("CHECK DUPE");
        tokio::time::delay_for(tokio::time::Duration::new(5, 0)).await;

        let mut diagnostics = Diagnostics::new();
        let mut names = HashSet::new();
        for (name, node) in ctx.current_module().await.exported_declarations().await {
            if names.contains(&name) {
                diagnostics.push(DuplicateExport(name, node));
            } else {
                names.insert(name);
            }
        }
        diagnostics
    }
}
