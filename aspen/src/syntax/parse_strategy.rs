use crate::syntax::ParseResult::{Failed, Succeeded};
use crate::syntax::{Expected, ParseResult, Parser};
use crate::{Diagnostic, Diagnostics};
use std::cmp::Ordering;
use std::marker::PhantomData;
use std::sync::Arc;

#[async_trait]
pub trait ParseStrategy<T>
where
    T: 'static,
    Self: Sized + Send,
{
    fn describe(&self) -> String;

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
    fn describe(&self) -> String {
        self.from.describe()
    }

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
    fn describe(&self) -> String {
        format!("multiple {}", self.strategy.describe())
    }

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
    fn describe(&self) -> String {
        format!("{} or {}", self.a.describe(), self.b.describe())
    }

    async fn parse(self, parser: &mut Parser) -> ParseResult<T> {
        let description = self.describe();

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

        match (
            a_result,
            b_result,
            a_parser.offset().cmp(&b_parser.offset()),
        ) {
            (ParseResult::Failed(d), ParseResult::Failed(_), Ordering::Greater) => {
                *parser = a_parser;
                ParseResult::Failed(d)
            }

            (ParseResult::Failed(_), ParseResult::Failed(d), Ordering::Less) => {
                *parser = b_parser;
                ParseResult::Failed(d)
            }

            (ParseResult::Failed(ad), ParseResult::Failed(bd), Ordering::Equal) => {
                let mut all_diagnostics: Vec<Arc<dyn Diagnostic>> =
                    ad.into_iter().chain(bd.into_iter()).collect();
                all_diagnostics.sort_by(|a, b| a.range().start.cmp(&b.range().end));
                let first_diagnostic = &all_diagnostics[0];

                ParseResult::Failed(
                    vec![Arc::new(Expected(
                        description,
                        first_diagnostic.source().clone(),
                        first_diagnostic.range().clone(),
                    )) as Arc<dyn Diagnostic>]
                    .into(),
                )
            }

            (ParseResult::Succeeded(d, t), ParseResult::Failed(_), _)
            | (ParseResult::Failed(_), ParseResult::Succeeded(d, t), _) => {
                ParseResult::Succeeded(d, t)
            }

            (a_result, b_result, _) => {
                if a_result > b_result {
                    *parser = a_parser;
                    a_result
                } else {
                    *parser = b_parser;
                    b_result
                }
            }
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
    fn describe(&self) -> String {
        self.strategy.describe()
    }

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
