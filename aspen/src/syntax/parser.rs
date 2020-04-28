use crate::syntax::{Node, Node::*, Token, TokenCursor, TokenKind as T};
use std::sync::Arc;

pub struct Parser {
    tokens: TokenCursor,
    diagnostics: Vec<String>,
}

impl Parser {
    pub fn new(tokens: Arc<Vec<Arc<Token>>>) -> Parser {
        Parser {
            tokens: TokenCursor::new(tokens),
            diagnostics: vec![],
        }
    }

    pub fn split(&self) -> Parser {
        Parser {
            tokens: self.tokens.split(),
            diagnostics: self.diagnostics.clone(),
        }
    }

    fn parse_many<F: Fn(&mut Self) -> Arc<Node>>(&mut self, f: F) -> Vec<Arc<Node>> {
        let mut nodes = vec![];

        while !self.tokens.is_at_end() {
            nodes.push(f(self));
        }

        nodes
    }

    fn unknown<S: Into<String>>(&mut self, message: S) -> Arc<Node> {
        self.diagnostics.push(message.into());
        Arc::new(Unknown(self.tokens.take()))
    }

    pub async fn parse_module(&mut self) -> Arc<Node> {
        Arc::new(Module {
            declarations: self.parse_many(Self::parse_declaration),
        })
    }

    pub fn parse_declaration(&mut self) -> Arc<Node> {
        if self.tokens.sees(T::ObjectKeyword) {
            self.parse_object_declaration()
        } else {
            self.unknown("Expected declaration.")
        }
    }

    pub fn parse_object_declaration(&mut self) -> Arc<Node> {
        Arc::new(ObjectDeclaration {
            keyword: self.tokens.take(),
            symbol: self.parse_symbol(),
        })
    }

    pub fn parse_symbol(&mut self) -> Arc<Node> {
        Arc::new(Symbol(self.tokens.take()))
    }
}
