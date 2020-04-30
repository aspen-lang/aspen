use crate::syntax::ParseResult::*;
use crate::syntax::{
    Lexer, Node, NodeKind, ParseMany, ParseResult, ParseStrategy, Token, TokenCursor, TokenKind,
};
use crate::{Diagnostics, Expected, Source};
use std::sync::Arc;

pub struct Parser {
    source: Arc<Source>,
    tokens: TokenCursor,
}

impl Parser {
    pub fn new(source: Arc<Source>) -> Parser {
        let tokens = Lexer::tokenize(&source);

        Parser {
            source,
            tokens: TokenCursor::new(tokens),
        }
    }

    pub fn split(&self) -> Parser {
        Parser {
            source: self.source.clone(),
            tokens: self.tokens.split(),
        }
    }

    pub async fn parse_module(&mut self) -> (Arc<Node>, Diagnostics) {
        match ParseModule.parse(self).await {
            Succeeded(d, t) => (t, d),

            Failed(d) => (
                self.node(NodeKind::Module {
                    declarations: vec![],
                }),
                d,
            ),
        }
    }

    pub fn fail_expecting<S: Into<String>, T>(&mut self, message: S) -> ParseResult<T> {
        ParseResult::fail(self.expected(message))
    }

    pub fn expect<S: Into<String>>(
        &mut self,
        kind: TokenKind,
        message: S,
    ) -> ParseResult<Arc<Token>> {
        if self.tokens.peek().kind == kind {
            Succeeded(Diagnostics::new(), self.tokens.take())
        } else {
            self.fail_expecting(message)
        }
    }

    pub fn expected<S: Into<String>>(&mut self, message: S) -> Expected {
        Expected(message.into(), self.tokens.clone_next_insignificant())
    }

    pub fn expect_optional_period(&mut self, diagnostics: &mut Diagnostics) -> Option<Arc<Token>> {
        if self.tokens.sees(TokenKind::Period) {
            Some(self.tokens.take())
        } else {
            diagnostics.push(self.expected("period"));
            None
        }
    }

    pub fn node(&self, kind: NodeKind) -> Arc<Node> {
        Node::new(self.source.clone(), kind)
    }
}

struct ParseModule;

#[async_trait]
impl ParseStrategy<Arc<Node>> for ParseModule {
    async fn parse(self, parser: &mut Parser) -> ParseResult<Arc<Node>> {
        let mut diagnostics = Diagnostics::new();
        let mut declarations = vec![];

        let mut encountered_error = false;
        while !parser.tokens.is_at_end() {
            match ParseMany::of(ParseDeclaration)
                .at_least_one()
                .parse(parser)
                .await
                .collect_diagnostics(&mut diagnostics)
            {
                Some(de) => {
                    declarations.extend(de);
                    encountered_error = false;
                }
                None => {
                    if !encountered_error {
                        diagnostics.push(parser.expected(format!("a declaration")));
                    }
                    parser.tokens.skip();
                    encountered_error = true;
                }
            }
        }
        Succeeded(diagnostics, parser.node(NodeKind::Module { declarations }))
    }
}

#[derive(Clone)]
struct ParseDeclaration;

#[async_trait]
impl ParseStrategy<Arc<Node>> for ParseDeclaration {
    async fn parse(self, parser: &mut Parser) -> ParseResult<Arc<Node>> {
        ParseObjectDeclaration
            .or(ParseClassDeclaration)
            .parse(parser)
            .await
    }
}

struct ParseObjectDeclaration;

#[async_trait]
impl ParseStrategy<Arc<Node>> for ParseObjectDeclaration {
    async fn parse(self, parser: &mut Parser) -> ParseResult<Arc<Node>> {
        parser
            .expect(TokenKind::ObjectKeyword, "object declaration")
            .and_then(async move |keyword| {
                ParseSymbol
                    .maybe()
                    .parse(parser)
                    .await
                    .and_then(async move |symbol| {
                        let mut diagnostics = Diagnostics::new();

                        if let None = symbol {
                            diagnostics.push(parser.expected("object name"));
                        }

                        let period = parser.expect_optional_period(&mut diagnostics);

                        Succeeded(
                            diagnostics,
                            parser.node(NodeKind::ObjectDeclaration {
                                keyword,
                                symbol,
                                period,
                            }),
                        )
                    })
                    .await
            })
            .await
    }
}

struct ParseClassDeclaration;

#[async_trait]
impl ParseStrategy<Arc<Node>> for ParseClassDeclaration {
    async fn parse(self, parser: &mut Parser) -> ParseResult<Arc<Node>> {
        parser
            .expect(TokenKind::ClassKeyword, "class declaration")
            .and_then(async move |keyword| {
                ParseSymbol
                    .maybe()
                    .parse(parser)
                    .await
                    .and_then(async move |symbol| {
                        let mut diagnostics = Diagnostics::new();

                        if let None = symbol {
                            diagnostics.push(parser.expected("class name"));
                        }

                        let period = parser.expect_optional_period(&mut diagnostics);

                        Succeeded(
                            diagnostics,
                            parser.node(NodeKind::ClassDeclaration {
                                keyword,
                                symbol,
                                period,
                            }),
                        )
                    })
                    .await
            })
            .await
    }
}

struct ParseSymbol;

#[async_trait]
impl ParseStrategy<Arc<Node>> for ParseSymbol {
    async fn parse(self, parser: &mut Parser) -> ParseResult<Arc<Node>> {
        if !parser.tokens.sees(TokenKind::Identifier) {
            parser.fail_expecting("symbol")
        } else {
            Succeeded(
                Diagnostics::new(),
                Node::new(
                    parser.source.clone(),
                    NodeKind::Symbol(parser.tokens.take()),
                ),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Source;

    #[tokio::test]
    async fn empty_module() {
        let source = Source::new("test:empty", "");
        let mut parser = Parser::new(source);
        parser.parse_module().await;
    }

    #[tokio::test]
    async fn single_object_declaration() {
        let source = Source::new("test:empty", "object Example.");
        let mut parser = Parser::new(source);
        let (module, _) = parser.parse_module().await;

        match &module.kind {
            NodeKind::Module { declarations } => {
                assert_eq!(declarations.len(), 1);
            }
            n => panic!("expected a module but got: {:?}", n),
        }
    }

    #[tokio::test]
    async fn two_object_declarations() {
        let source = Source::new("test:empty", "object A. object B.");
        let mut parser = Parser::new(source);
        let (module, _) = parser.parse_module().await;

        assert_eq!(
            format!("{:#?}", module),
            r#"
            Module {
                declarations: [
                    ObjectDeclaration {
                        keyword: ObjectKeyword,
                        symbol: Some(
                            Symbol(
                                Identifier "A",
                            ),
                        ),
                        period: Some(
                            Period,
                        ),
                    },
                    ObjectDeclaration {
                        keyword: ObjectKeyword,
                        symbol: Some(
                            Symbol(
                                Identifier "B",
                            ),
                        ),
                        period: Some(
                            Period,
                        ),
                    },
                ],
            }
        "#
            .trim()
            .replace("\n            ", "\n")
        );
    }
}
