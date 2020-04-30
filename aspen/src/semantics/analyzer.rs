use crate::semantics::{Host, Module};
use crate::syntax::{Navigator, Node, NodeKind};
use crate::{Diagnostics, DuplicateExport, URI};
use futures::future;
use std::borrow::BorrowMut;
use std::collections::HashSet;
use std::iter::FromIterator;
use std::sync::Arc;
use tokio::sync::Mutex;

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
pub trait Analyzer
where
    Self: Send + Sized,
{
    type Output: Send;

    async fn analyze(self, ctx: AnalysisContext) -> Self::Output;

    fn and<B: Analyzer<Output = Self::Output>>(self, b: B) -> MergeTwo<Self, B> {
        MergeTwo::both(self, b)
    }
}

pub struct Memo<A: Analyzer> {
    mutex: Mutex<Option<A::Output>>,
    analyzer: A,
}

impl<A: Analyzer> Memo<A> {
    pub fn of(analyzer: A) -> Memo<A> {
        Memo {
            mutex: Mutex::new(None),
            analyzer,
        }
    }
}

#[async_trait]
impl<A> Analyzer for &Memo<A>
where
    A: Analyzer + Clone + Sync + Send,
    A::Output: Clone,
{
    type Output = A::Output;

    async fn analyze(self, ctx: AnalysisContext) -> A::Output {
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
impl<A> Analyzer for &Once<A>
where
    A: Analyzer,
    A::Output: Default,
{
    type Output = A::Output;

    async fn analyze(self, ctx: AnalysisContext) -> A::Output {
        let mut lock = self.analyzer.lock().await;
        let opt: &mut Option<A> = lock.borrow_mut();

        if opt.is_none() {
            return Default::default();
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
        MergeTwo { a, b }
    }
}

#[async_trait]
impl<T, A, B> Analyzer for MergeTwo<A, B>
where
    T: 'static + FromIterator<T> + Send,
    A: Analyzer<Output = T>,
    B: Analyzer<Output = T>,
{
    type Output = T;

    async fn analyze(self, ctx: AnalysisContext) -> T {
        let (a, b) = future::join(self.a.analyze(ctx.clone()), self.b.analyze(ctx)).await;

        vec![a, b].into_iter().collect()
    }
}

pub struct ExportedDeclarations;

#[async_trait]
impl Analyzer for &ExportedDeclarations {
    type Output = Vec<(String, Arc<Node>)>;

    async fn analyze(self, ctx: AnalysisContext) -> Self::Output {
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

pub struct CheckForDuplicateExports;

#[async_trait]
impl Analyzer for &CheckForDuplicateExports {
    type Output = Diagnostics;

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
