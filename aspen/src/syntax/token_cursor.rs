use crate::syntax::{Token, TokenKind, TokenKind::*};
use crate::Range;
use std::fmt;
use std::sync::Arc;

pub struct TokenCursor {
    tokens: Arc<Vec<Arc<Token>>>,
    offset: usize,
    insignificant_offset: usize,
}

impl TokenCursor {
    pub fn new(tokens: Arc<Vec<Arc<Token>>>) -> TokenCursor {
        if tokens.len() == 0 {
            panic!("cannot construct a cursor from an empty list of tokens");
        }

        let mut cursor = TokenCursor {
            tokens,
            offset: 0,
            insignificant_offset: 0,
        };

        cursor.move_past_whitespace();

        cursor
    }

    pub fn offset(&self) -> usize {
        self.offset
    }

    fn move_past_whitespace(&mut self) {
        while self.sees(Whitespace) {
            self.offset += 1;
        }
    }

    pub fn peek(&self) -> &Token {
        self.tokens[self.offset].as_ref()
    }

    pub fn sees(&self, kind: TokenKind) -> bool {
        self.peek().kind == kind
    }

    pub fn clone_next_insignificant(&self) -> Arc<Token> {
        self.tokens[self.insignificant_offset].clone()
    }

    pub fn take(&mut self) -> Arc<Token> {
        let token = self.tokens[self.offset].clone();

        if self.offset < self.tokens.len() - 1 {
            self.offset += 1;
        }
        self.insignificant_offset = self.offset;

        self.move_past_whitespace();

        token
    }

    pub fn skip(&mut self) {
        self.take();
    }

    pub fn split(&self) -> TokenCursor {
        TokenCursor {
            tokens: self.tokens.clone(),
            offset: self.offset,
            insignificant_offset: self.insignificant_offset,
        }
    }

    pub fn is_at_end(&self) -> bool {
        self.sees(EOF)
    }

    pub fn range(&self) -> Range {
        self.peek().range.clone()
    }
}

impl fmt::Debug for TokenCursor {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "TokenCursor @ {}/{}", self.offset, self.tokens.len())
    }
}
