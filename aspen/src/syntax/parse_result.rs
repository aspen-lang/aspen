use crate::{Diagnostic, Diagnostics};
use std::cmp::Ordering;
use std::future::Future;

pub enum ParseResult<T> {
    Failed(Diagnostics),
    Succeeded(Diagnostics, T),
}

impl<T> ParseResult<T> {
    pub fn map<U, F: FnOnce(T) -> U>(self, f: F) -> ParseResult<U> {
        match self {
            ParseResult::Succeeded(d, t) => ParseResult::Succeeded(d, f(t)),
            ParseResult::Failed(d) => ParseResult::Failed(d),
        }
    }

    pub async fn and_then<U, R: Future<Output = ParseResult<U>>, F: FnOnce(T) -> R>(
        self,
        f: F,
    ) -> ParseResult<U> {
        match self {
            ParseResult::Succeeded(d, t) => match f(t).await {
                ParseResult::Succeeded(dd, u) => ParseResult::Succeeded(d.and(dd), u),
                ParseResult::Failed(dd) => ParseResult::Failed(d.and(dd)),
            },
            ParseResult::Failed(d) => ParseResult::Failed(d),
        }
    }

    pub fn unwrap(self) -> T {
        match self {
            ParseResult::Succeeded(d, t) => {
                if d.is_empty() {
                    t
                } else {
                    panic!("{:?}", d)
                }
            }
            ParseResult::Failed(d) => panic!("{:?}", d),
        }
    }

    pub fn fail<D: Diagnostic + 'static>(diagnostic: D) -> ParseResult<T> {
        let b: Box<dyn Diagnostic> = Box::new(diagnostic);
        ParseResult::Failed(b.into())
    }

    pub fn collect_diagnostics(self, diagnostics: &mut Diagnostics) -> Option<T> {
        match self {
            ParseResult::Succeeded(ds, t) => {
                diagnostics.push_all(ds);
                Some(t)
            }
            ParseResult::Failed(ds) => {
                diagnostics.push_all(ds);
                None
            }
        }
    }
}

impl<T> PartialEq for ParseResult<T> {
    fn eq(&self, _other: &Self) -> bool {
        false
    }
}

impl<T> PartialOrd for ParseResult<T> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        use ParseResult::*;
        Some(match (self, other) {
            (Succeeded(ad, _), Succeeded(bd, _)) => {
                if bd.looks_more_promising_than(&ad) {
                    Ordering::Less
                } else {
                    Ordering::Greater
                }
            }

            (Succeeded(_, _), Failed(_)) => Ordering::Greater,

            (Failed(_), Succeeded(_, _)) => Ordering::Less,

            (Failed(ad), Failed(bd)) => {
                if bd.looks_more_promising_than(&ad) {
                    Ordering::Less
                } else {
                    Ordering::Greater
                }
            }
        })
    }
}
