use self::TokenKind::*;
use crate::source::{self, IntoRange, Source};
use std::fmt;
use std::sync::Arc;

pub struct Token {
    pub range: source::Range,
    pub source: Arc<Source>,
    pub kind: TokenKind,
}

impl Token {
    pub fn new<R>(kind: TokenKind, source: &Arc<Source>, range: R) -> Arc<Token>
    where
        R: IntoRange,
    {
        let range = range.into_range(source.as_ref());
        let source = source.clone();

        Arc::new(Token {
            range,
            source,
            kind,
        })
    }

    pub fn lexeme(&self) -> &str {
        self.source.slice(&self.range)
    }
}

impl PartialEq for Token {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.source, &other.source)
            && self.range == other.range
            && self.kind == other.kind
    }
}

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.kind {
            Unknown | Identifier => write!(f, "{:?} {:?}", self.kind, self.lexeme()),

            _ => write!(f, "{:?}", self.kind),
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum TokenKind {
    Unknown,
    EOF,
    Whitespace,

    Period,

    IntegerLiteral(i128, bool),
    FloatLiteral(f64, bool),

    Identifier,

    ObjectKeyword,
}
