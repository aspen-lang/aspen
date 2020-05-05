use crate::syntax::ParseResult::{Failed, Succeeded};
use crate::syntax::{ParseResult, Parser};
use crate::Diagnostics;
use std::marker::PhantomData;

#[async_trait]
pub trait ParseStrategy<T>
where
    Self: Sized + Send,
{
    async fn parse(self, parser: &mut Parser) -> ParseResult<T>;

    fn or<S: ParseStrategy<T>>(self, other: S) -> ParseEither<Self, S> {
        ParseEither { a: self, b: other }
    }

    fn maybe(self) -> MaybeParse<Self> {
        MaybeParse::some(self)
    }

    fn map<U, F: FnOnce(T) -> U>(self, f: F) -> MapParse<Self, T, F> {
        MapParse {
            _t: PhantomData,
            from: self,
            via: f,
        }
    }
}

pub struct MapParse<S, T, F> {
    _t: PhantomData<T>,
    from: S,
    via: F,
}

#[async_trait]
impl<S, T, F, U> ParseStrategy<U> for MapParse<S, T, F>
where
    U: 'static + Send + Sync,
    T: 'static + Send + Sync,
    S: ParseStrategy<T>,
    F: FnOnce(T) -> U + Send + Sync,
{
    async fn parse(self, parser: &mut Parser) -> ParseResult<U> {
        self.from.parse(parser).await.map(self.via)
    }
}

pub struct ParseMany<S> {
    strategy: S,
    at_least_one: bool,
}

impl<S> ParseMany<S> {
    pub fn of(strategy: S) -> ParseMany<S> {
        ParseMany {
            strategy,
            at_least_one: false,
        }
    }

    pub fn at_least_one(mut self) -> Self {
        self.at_least_one = true;
        self
    }
}

#[async_trait]
impl<T: 'static, S> ParseStrategy<Vec<T>> for ParseMany<S>
where
    S: ParseStrategy<T> + Clone + Sync,
    T: Send,
{
    async fn parse(self, parser: &mut Parser) -> ParseResult<Vec<T>> {
        let mut result = vec![];
        let mut diagnostics = Diagnostics::new();
        while let Succeeded(d, t) = self.strategy.clone().parse(parser).await {
            result.push(t);
            diagnostics.push_all(d);
        }
        if self.at_least_one && result.is_empty() {
            Failed(diagnostics)
        } else {
            Succeeded(diagnostics, result)
        }
    }
}

pub struct FailedParse;

#[async_trait]
impl<T: 'static> ParseStrategy<T> for FailedParse {
    async fn parse(self, _parser: &mut Parser) -> ParseResult<T> {
        ParseResult::Failed(Diagnostics::new())
    }
}

pub struct SucceededParse<T>(pub T);

#[async_trait]
impl<T: Send + 'static> ParseStrategy<T> for SucceededParse<T> {
    async fn parse(self, _parser: &mut Parser) -> ParseResult<T> {
        ParseResult::Succeeded(Diagnostics::new(), self.0)
    }
}

pub struct ParseEither<A, B> {
    a: A,
    b: B,
}

#[async_trait]
impl<A: 'static, B: 'static, T: 'static> ParseStrategy<T> for ParseEither<A, B>
where
    T: Send,
    A: ParseStrategy<T>,
    B: ParseStrategy<T>,
{
    async fn parse(self, parser: &mut Parser) -> ParseResult<T> {
        let a = self.a;
        let b = self.b;

        let mut a_parser = parser.split();
        let mut b_parser = parser.split();

        let a_join = tokio::spawn(async move {
            let r = a.parse(&mut a_parser).await;
            (r, a_parser)
        });

        let b_join = tokio::spawn(async move {
            let r = b.parse(&mut b_parser).await;
            (r, b_parser)
        });

        let (a_result, a_parser) = a_join.await.unwrap();
        let (b_result, b_parser) = b_join.await.unwrap();

        if a_result > b_result {
            *parser = a_parser;
            a_result
        } else {
            *parser = b_parser;
            b_result
        }
    }
}

pub struct MaybeParse<S> {
    strategy: S,
}

impl<S> MaybeParse<S> {
    pub fn some(strategy: S) -> MaybeParse<S> {
        MaybeParse { strategy }
    }
}

#[async_trait]
impl<T: 'static, S> ParseStrategy<Option<T>> for MaybeParse<S>
where
    S: ParseStrategy<T> + Sync,
    T: Send,
{
    async fn parse(self, parser: &mut Parser) -> ParseResult<Option<T>> {
        let mut sub_parser = parser.split();
        match self.strategy.parse(&mut sub_parser).await {
            Succeeded(d, t) => {
                *parser = sub_parser;
                Succeeded(d, Some(t))
            }
            Failed(_) => Succeeded(Diagnostics::new(), None),
        }
    }
}
