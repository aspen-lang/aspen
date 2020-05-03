use crate::semantics::{Host, Module};
use crate::syntax::Navigator;
use crate::URI;
use futures::future;
use std::borrow::BorrowMut;
use std::collections::HashMap;
use std::hash::Hash;
use std::iter::FromIterator;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct AnalysisContext<I> {
    pub uri: URI,
    pub host: Host,
    pub navigator: Arc<Navigator>,
    pub input: I,
}

impl<I> AnalysisContext<I> {
    pub async fn current_module(&self) -> Arc<Module> {
        self.host.get(&self.uri).await.unwrap()
    }
}

#[async_trait]
pub trait Analyzer
where
    Self: Send + Sized,
{
    type Input: Send;
    type Output: Send;

    async fn analyze(self, ctx: AnalysisContext<Self::Input>) -> Self::Output;

    fn and<B: Analyzer<Output = Self::Output>>(self, b: B) -> MergeTwo<Self, B> {
        MergeTwo::both(self, b)
    }
}

pub struct Memo<A: Analyzer> {
    mutex: Mutex<HashMap<A::Input, A::Output>>,
    analyzer: A,
}

impl<A: Analyzer> Memo<A> {
    pub fn of(analyzer: A) -> Memo<A> {
        Memo {
            mutex: Mutex::new(HashMap::new()),
            analyzer,
        }
    }
}

#[async_trait]
impl<A> Analyzer for &Memo<A>
where
    A: Analyzer + Clone + Sync + Send,
    A::Input: Hash + Eq + Clone,
    A::Output: Clone,
{
    type Input = A::Input;
    type Output = A::Output;

    async fn analyze(self, ctx: AnalysisContext<Self::Input>) -> A::Output {
        let mut map = self.mutex.lock().await;

        match map.get(&ctx.input) {
            Some(t) => return t.clone(),
            None => {}
        }
        let analyzer = self.analyzer.clone();
        let input = ctx.input.clone();
        let t = analyzer.analyze(ctx).await;
        map.insert(input, t.clone());
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
    A: Analyzer<Input = ()>,
    A::Output: Default,
{
    type Input = ();
    type Output = A::Output;

    async fn analyze(self, ctx: AnalysisContext<()>) -> A::Output {
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
impl<I, O, A, B> Analyzer for MergeTwo<A, B>
where
    I: 'static + Clone + Send + Sync,
    O: 'static + FromIterator<O> + Send,
    A: Analyzer<Input = I, Output = O>,
    B: Analyzer<Input = I, Output = O>,
{
    type Input = I;
    type Output = O;

    async fn analyze(self, ctx: AnalysisContext<I>) -> O {
        let (a, b) = future::join(self.a.analyze(ctx.clone()), self.b.analyze(ctx)).await;

        vec![a, b].into_iter().collect()
    }
}
