//! # Syntax
//! This module is concerned with specifying the syntax and grammar of
//! the Aspen language, as well as implementing the parsing of that
//! grammar.

mod lexer;
mod node;
mod parse_result;
mod parse_strategy;
mod parser;
mod token;
mod token_cursor;

pub use self::lexer::*;
pub use self::node::*;
pub use self::parse_result::*;
pub use self::parse_strategy::*;
pub use self::parser::*;
pub use self::token::*;
pub use self::token_cursor::*;

use crate::{Diagnostics, Source};
use std::iter::IntoIterator;
use std::sync::Arc;
use tokio::stream::StreamExt;

pub async fn parse_module(source: Arc<Source>) -> (Arc<Node>, Diagnostics) {
    let tokens = Lexer::tokenize(&source);
    Parser::new(tokens).parse_module().await
}

pub async fn parse_modules(sources: Vec<Arc<Source>>) -> (Vec<Arc<Node>>, Diagnostics) {
    let (sender, receiver) = tokio::sync::mpsc::channel(sources.len());

    for source in sources.into_iter() {
        let mut sender = sender.clone();
        tokio::spawn(async move {
            sender.send(parse_module(source).await).await.unwrap();
        });
    }

    drop(sender);

    let mut diagnostics = Diagnostics::new();

    (
        receiver
            .map(|(node, d)| {
                diagnostics.push_all(d);
                node
            })
            .collect()
            .await,
        diagnostics,
    )
}
