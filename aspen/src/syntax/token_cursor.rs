use crate::syntax::{Token, TokenKind, TokenKind::*};
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

        TokenCursor { tokens, offset: 0 }
    }

    pub fn peek(&self) -> &Token {
        self.tokens[self.offset].as_ref()
    }

    pub fn sees(&self, kind: TokenKind) -> bool {
        self.peek().kind == kind
    }

    pub fn take(&mut self) -> Arc<Token> {
        let token = self.tokens[self.offset].clone();

        if self.offset < self.tokens.len() - 1 {
            self.offset += 1;
        }

        token
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
}
