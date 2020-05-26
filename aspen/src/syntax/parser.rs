use crate::syntax::ParseResult::*;
use crate::syntax::*;
use crate::{Diagnostics, Source, SourceKind};
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

    pub async fn parse(&mut self) -> (Arc<Root>, Diagnostics) {
        let result = ParseRoot.parse(self).await;

        match result {
            Succeeded(mut d, t) => {
                if !self.tokens.is_at_end() {
                    d.push(self.expected("end"));
                }
                (t, d)
            }

            Failed(d) => (
                Arc::new(Root::Module(Arc::new(Module {
                    source: self.source.clone(),
                    declarations: vec![],
                }))),
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
        let token = self.tokens.clone_next_insignificant();
        Expected(message.into(), token.source.clone(), token.range.clone())
    }

    pub fn expect_optional_period(&mut self, diagnostics: &mut Diagnostics) -> Option<Arc<Token>> {
        if self.tokens.sees(TokenKind::Period) {
            Some(self.tokens.take())
        } else {
            diagnostics.push(self.expected("period"));
            None
        }
    }

    pub fn offset(&self) -> usize {
        self.tokens.offset()
    }
}

struct ParseRoot;

#[async_trait]
impl ParseStrategy<Arc<Root>> for ParseRoot {
    fn describe(&self) -> String {
        "source".into()
    }

    async fn parse(self, parser: &mut Parser) -> ParseResult<Arc<Root>> {
        match parser.source.kind {
            SourceKind::Inline => ParseInline.map(Root::Inline).parse(parser),
            SourceKind::Module => ParseModule.map(Root::Module).parse(parser),
        }
        .await
        .map(Arc::new)
    }
}

struct ParseInline;

#[async_trait]
impl ParseStrategy<Arc<Inline>> for ParseInline {
    fn describe(&self) -> String {
        "inline code".into()
    }

    async fn parse(self, parser: &mut Parser) -> ParseResult<Arc<Inline>> {
        ParseDeclaration
            .map(Inline::Declaration)
            .or(ParseExpression.map(|e| Inline::Expression(e, None)))
            .parse(parser)
            .await
            .and_then(async move |inline| {
                if let Inline::Expression(e, _) = inline {
                    let mut diagnostics = Diagnostics::new();
                    let period = parser.expect_optional_period(&mut diagnostics);
                    Inline::Expression(e, period).into()
                } else {
                    inline.into()
                }
            })
            .await
            .map(Arc::new)
    }
}

struct ParseModule;

#[async_trait]
impl ParseStrategy<Arc<Module>> for ParseModule {
    fn describe(&self) -> String {
        "declarations".into()
    }

    async fn parse(self, parser: &mut Parser) -> ParseResult<Arc<Module>> {
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
        Succeeded(
            diagnostics,
            Arc::new(Module {
                source: parser.source.clone(),
                declarations,
            }),
        )
    }
}

#[derive(Clone)]
struct ParseDeclaration;

#[async_trait]
impl ParseStrategy<Arc<Declaration>> for ParseDeclaration {
    fn describe(&self) -> String {
        "object declaration".into()
    }

    async fn parse(self, parser: &mut Parser) -> ParseResult<Arc<Declaration>> {
        ParseObjectDeclaration
            .map(Declaration::Object)
            .parse(parser)
            .await
            .map(Arc::new)
    }
}

struct ParseObjectDeclaration;

#[async_trait]
impl ParseStrategy<Arc<ObjectDeclaration>> for ParseObjectDeclaration {
    fn describe(&self) -> String {
        "object declaration".into()
    }

    async fn parse(self, parser: &mut Parser) -> ParseResult<Arc<ObjectDeclaration>> {
        parser
            .expect(TokenKind::ObjectKeyword, "object declaration")
            .and_then(async move |keyword| {
                ParseSymbol
                    .parse(parser)
                    .await
                    .and_then(async move |symbol| {
                        let mut diagnostics = Diagnostics::new();

                        if parser.tokens.sees(TokenKind::OpenCurly) {
                            ParseObjectBody.parse(parser).await.map(|body| {
                                Arc::new(ObjectDeclaration {
                                    source: parser.source.clone(),
                                    keyword,
                                    symbol,
                                    period: None,
                                    body: Some(body),
                                })
                            })
                        } else {
                            let period = parser.expect_optional_period(&mut diagnostics);

                            Succeeded(
                                diagnostics,
                                Arc::new(ObjectDeclaration {
                                    source: parser.source.clone(),
                                    keyword,
                                    symbol,
                                    period,
                                    body: None,
                                }),
                            )
                        }
                    })
                    .await
            })
            .await
    }
}

struct ParseObjectBody;

#[async_trait]
impl ParseStrategy<Arc<ObjectBody>> for ParseObjectBody {
    fn describe(&self) -> String {
        "object body".into()
    }

    async fn parse(self, parser: &mut Parser) -> ParseResult<Arc<ObjectBody>> {
        parser
            .expect(TokenKind::OpenCurly, "object body")
            .and_then(async move |open_curly| {
                let mut diagnostics = Diagnostics::new();

                let close_curly = parser
                    .expect(TokenKind::CloseCurly, "end of object body")
                    .collect_diagnostics(&mut diagnostics);

                Succeeded(
                    diagnostics,
                    Arc::new(ObjectBody {
                        source: parser.source.clone(),
                        open_curly,
                        close_curly,
                    }),
                )
            })
            .await
    }
}

struct ParseTypeExpression;

#[async_trait]
impl ParseStrategy<Arc<TypeExpression>> for ParseTypeExpression {
    fn describe(&self) -> String {
        "type expression".into()
    }

    async fn parse(self, parser: &mut Parser) -> ParseResult<Arc<TypeExpression>> {
        ParseReferenceTypeExpression
            .parse(parser)
            .await
            .map(TypeExpression::Reference)
            .map(Arc::new)
    }
}

struct ParseReferenceTypeExpression;

#[async_trait]
impl ParseStrategy<Arc<ReferenceTypeExpression>> for ParseReferenceTypeExpression {
    fn describe(&self) -> String {
        "type reference".into()
    }

    async fn parse(self, parser: &mut Parser) -> ParseResult<Arc<ReferenceTypeExpression>> {
        ParseSymbol.parse(parser).await.map(|symbol| {
            Arc::new(ReferenceTypeExpression {
                source: parser.source.clone(),
                symbol,
            })
        })
    }
}

struct ParseSymbol;

#[async_trait]
impl ParseStrategy<Arc<Symbol>> for ParseSymbol {
    fn describe(&self) -> String {
        "symbol".into()
    }

    async fn parse(self, parser: &mut Parser) -> ParseResult<Arc<Symbol>> {
        if !parser.tokens.sees(TokenKind::Identifier) {
            parser.fail_expecting("symbol")
        } else {
            Succeeded(
                Diagnostics::new(),
                Arc::new(Symbol {
                    source: parser.source.clone(),
                    identifier: parser.tokens.take(),
                }),
            )
        }
    }
}

struct ParseExpression;

#[async_trait]
impl ParseStrategy<Arc<Expression>> for ParseExpression {
    fn describe(&self) -> String {
        "expression".into()
    }

    async fn parse(self, parser: &mut Parser) -> ParseResult<Arc<Expression>> {
        ParseTerm
            .parse(parser)
            .await
            .and_then(async move |mut expression| {
                let mut diagnostics = Diagnostics::new();
                while let Succeeded(d, message) = ParseTerm.parse(parser).await {
                    diagnostics.push_all(d);
                    expression = Arc::new(Expression::MessageSend(Arc::new(MessageSend {
                        source: parser.source.clone(),
                        receiver: expression,
                        message,
                    })));
                }

                Succeeded(diagnostics, expression)
            })
            .await
    }
}

struct ParseTerm;

#[async_trait]
impl ParseStrategy<Arc<Expression>> for ParseTerm {
    fn describe(&self) -> String {
        "expression".into()
    }

    async fn parse(self, parser: &mut Parser) -> ParseResult<Arc<Expression>> {
        match &parser.tokens.peek().kind {
            TokenKind::IntegerLiteral(_, _) => Succeeded(
                Diagnostics::new(),
                Arc::new(Expression::Integer(Arc::new(Integer {
                    source: parser.source.clone(),
                    literal: parser.tokens.take(),
                }))),
            ),
            TokenKind::FloatLiteral(_, _) => Succeeded(
                Diagnostics::new(),
                Arc::new(Expression::Float(Arc::new(Float {
                    source: parser.source.clone(),
                    literal: parser.tokens.take(),
                }))),
            ),
            TokenKind::NullaryAtom => Succeeded(
                Diagnostics::new(),
                Arc::new(Expression::NullaryAtom(Arc::new(NullaryAtomExpression {
                    source: parser.source.clone(),
                    atom: parser.tokens.take(),
                }))),
            ),
            TokenKind::Identifier => ParseReferenceExpression
                .map(Expression::Reference)
                .parse(parser)
                .await
                .map(Arc::new),
            _ => parser.fail_expecting("expression"),
        }
    }
}

struct ParseReferenceExpression;

#[async_trait]
impl ParseStrategy<Arc<ReferenceExpression>> for ParseReferenceExpression {
    fn describe(&self) -> String {
        "reference".into()
    }

    async fn parse(self, parser: &mut Parser) -> ParseResult<Arc<ReferenceExpression>> {
        ParseSymbol.parse(parser).await.map(|symbol| {
            Arc::new(ReferenceExpression {
                source: parser.source.clone(),
                symbol,
            })
        })
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
        parser.parse().await;
    }

    #[tokio::test]
    async fn single_object_declaration() {
        let source = Source::new("test:single-object-declaration", "object Example.");
        let mut parser = Parser::new(source);
        let (module, _) = parser.parse().await;

        assert_eq!(module.as_module().unwrap().declarations.len(), 1)
    }
}
