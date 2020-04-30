use crate::semantics::{Host, Module};
use crate::syntax::{Navigator, Node, NodeKind};
use crate::{Diagnostics, DuplicateExport, URI, Merge};
use futures::future;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::Mutex;
use std::borrow::BorrowMut;

#[derive(Clone)]
pub struct AnalysisContext {
    pub uri: URI,
    pub host: Host,
    pub navigator: Navigator<'static>,
}

impl AnalysisContext {
    pub async fn current_module(&self) -> Arc<Module> {
        self.host.get(&self.uri).await.unwrap()
    }
}

#[async_trait]
pub trait Analyzer<T> where Self: Send + Sized {
    async fn analyze(self, ctx: AnalysisContext) -> T;

    fn and<B: Analyzer<T>>(self, b: B) -> MergeTwo<Self, B> {
        MergeTwo::both(self, b)
    }
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
impl<A, T> Analyzer<T> for &Memo<A, T>
    where
        A: Analyzer<T> + Clone + Sync + Send,
        T: Clone + Send,
{
    async fn analyze(self, ctx: AnalysisContext) -> T {
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

pub struct Once<A> {
    analyzer: Mutex<Option<A>>,
}

impl<A> Once<A> {
    pub fn of(analyzer: A) -> Once<A> {
        Once {
            analyzer: Mutex::new(Some(analyzer)),
        }
    }
}

#[async_trait]
impl<A, T> Analyzer<T> for &Once<A>
where
    A: Analyzer<T>,
    T: 'static + Default,
{
    async fn analyze(self, ctx: AnalysisContext) -> T {
        let mut lock = self.analyzer.lock().await;
        let opt: &mut Option<A> = lock.borrow_mut();

        if opt.is_none() {
            return T::default();
        }

        let analyzer = std::mem::replace(opt, None).unwrap();
        analyzer.analyze(ctx).await
    }
}

pub struct MergeTwo<A, B> {
    a: A,
    b: B,
}

impl<A, B> MergeTwo<A, B> {
    pub fn both(a: A, b: B) -> MergeTwo<A, B> {
        MergeTwo {
            a,
            b,
        }
    }
}

#[async_trait]
impl<T, A, B> Analyzer<T> for MergeTwo<A, B>
where
    T: 'static + Merge + Send,
    A: Analyzer<T>,
    B: Analyzer<T>,
{
    async fn analyze(self, ctx: AnalysisContext) -> T {
        let (a, b) = future::join(
            self.a.analyze(ctx.clone()),
            self.b.analyze(ctx),
        ).await;

        T::merge(vec![a, b])
    }
}

#[derive(Clone)]
pub struct ExportedDeclarations;

#[async_trait]
impl Analyzer<Vec<(String, Arc<Node>)>> for ExportedDeclarations {
    async fn analyze(self, ctx: AnalysisContext) -> Vec<(String, Arc<Node>)> {
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
impl Analyzer<Diagnostics> for CheckForDuplicateExports {
    async fn analyze(self, ctx: AnalysisContext) -> Diagnostics {
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
