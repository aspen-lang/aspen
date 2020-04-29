use crate::syntax::{Token, TokenKind, TokenKind::*};
use crate::Range;
use std::fmt;
use std::sync::Arc;

pub struct TokenCursor {
    tokens: Arc<Vec<Arc<Token>>>,
    offset: usize,
}

impl TokenCursor {
    pub fn new(tokens: Arc<Vec<Arc<Token>>>) -> TokenCursor {
        if tokens.len() == 0 {
            panic!("cannot construct a cursor from an empty list of tokens");
        }

        let mut cursor = TokenCursor { tokens, offset: 0 };

        cursor.move_past_whitespace();

        cursor
    }

    fn move_past_whitespace(&mut self) {
        while self.sees(Whitespace) {
            self.skip();
        }
    }

    pub fn peek(&self) -> &Token {
        self.tokens[self.offset].as_ref()
    }

    pub fn sees(&self, kind: TokenKind) -> bool {
        self.peek().kind == kind
    }

    pub fn clone_next(&self) -> Arc<Token> {
        self.tokens[self.offset].clone()
    }

    pub fn take(&mut self) -> Arc<Token> {
        let token = self.tokens[self.offset].clone();

        if self.offset < self.tokens.len() - 1 {
            self.offset += 1;
        }

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
