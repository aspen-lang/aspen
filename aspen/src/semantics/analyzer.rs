use crate::semantics::{Host, Module};
use crate::syntax::Navigator;
use futures::future;
use std::borrow::BorrowMut;
use std::collections::HashMap;
use std::iter::FromIterator;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct AnalysisContext<I> {
    pub module: Arc<Module>,
    pub host: Host,
    pub navigator: Arc<Navigator>,
    pub input: I,
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

pub struct MemoOut<A: Analyzer> {
    mutex: Mutex<Option<A::Output>>,
    analyzer: A,
}

impl<A: Analyzer> MemoOut<A> {
    pub fn of(analyzer: A) -> MemoOut<A> {
        MemoOut {
            mutex: Mutex::new(None),
            analyzer,
        }
    }
}

#[async_trait]
impl<A> Analyzer for &MemoOut<A>
where
    A: Analyzer<Input = ()> + Clone + Sync + Send,
    A::Output: Clone,
{
    type Input = ();
    type Output = A::Output;

    async fn analyze(self, ctx: AnalysisContext<Self::Input>) -> A::Output {
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

pub struct Memo<A: Analyzer, K> {
    mutex: Mutex<HashMap<K, A::Output>>,
    analyzer: A,
}

impl<A: Analyzer, K> Memo<A, K> {
    pub fn of(analyzer: A) -> Memo<A, K> {
        Memo {
            mutex: Mutex::new(HashMap::new()),
            analyzer,
        }
    }
}

pub trait PtrAsUsize {
    fn ptr_as_usize(&self) -> usize;
}

impl<T> PtrAsUsize for Arc<T> {
    fn ptr_as_usize(&self) -> usize {
        self.as_ref() as *const _ as usize
    }
}

#[async_trait]
impl<A> Analyzer for &Memo<A, usize>
where
    A: Analyzer + Clone + Sync + Send,
    A::Output: Clone,
    A::Input: PtrAsUsize,
{
    type Input = A::Input;
    type Output = A::Output;

    async fn analyze(self, ctx: AnalysisContext<Self::Input>) -> A::Output {
        let key = ctx.input.ptr_as_usize();
        let mut map = self.mutex.lock().await;

        match map.get(&key) {
            Some(t) => return t.clone(),
            None => {}
        }
        let analyzer = self.analyzer.clone();
        let t = analyzer.analyze(ctx).await;
        map.insert(key, t.clone());
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
