//! # Syntax
//! This module is concerned with specifying the syntax and grammar of
//! the Aspen language, as well as implementing the parsing of that
//! grammar.

mod lexer;
mod node;
mod parser;
mod token;
mod token_cursor;

pub use self::lexer::*;
pub use self::node::*;
pub use self::parser::*;
pub use self::token::*;
pub use self::token_cursor::*;

use std::sync::Arc;
use crate::Source;
use tokio::stream::StreamExt;
use std::iter::IntoIterator;

pub async fn parse_module(source: Arc<Source>) -> Arc<Node> {
    let tokens = Lexer::tokenize(&source).await;
    Parser::new(tokens).parse_module().await
}

pub async fn parse_modules(sources: Vec<Arc<Source>>) -> Vec<Arc<Node>> {
    let (sender, receiver) = tokio::sync::mpsc::channel(sources.len());

    for source in sources.into_iter() {
        let mut sender = sender.clone();
        tokio::spawn(async move {
            sender.send(parse_module(source).await).await.unwrap();
        });
    }

    drop(sender);
    receiver.collect().await
}
