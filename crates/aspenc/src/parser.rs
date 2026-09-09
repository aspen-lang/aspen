use super::*;

struct Parser<'a, 'd> {
    tokens: Vec<Loc<Token<'a>>>,
    index: usize,
    eof: Pos,
    diagnostics: &'d mut Vec<Diagnostic>,
}

impl<'a, 'd> Parser<'a, 'd> {
    fn new(mut lexer: Lexer<'a>, diagnostics: &'d mut Vec<Diagnostic>) -> (Self, bool) {
        let tokens = lexer
            .by_ref()
            .filter(|t| !matches!(t.value, Token::Whitespace(_)))
            .collect();
        let invalid = !lexer.diagnostics.is_empty();
        diagnostics.append(&mut lexer.diagnostics);
        (
            Self {
                tokens,
                index: 0,
                eof: lexer.position(),
                diagnostics,
            },
            invalid,
        )
    }

    fn peek(&self) -> Option<Token<'a>> {
        self.tokens.get(self.index).map(|t| t.value)
    }
    fn span(&self) -> Span {
        self.tokens.get(self.index).map_or(
            Span {
                start: self.eof,
                end: self.eof,
            },
            |t| t.span,
        )
    }
    fn bump(&mut self) -> Loc<Token<'a>> {
        let t = self.tokens[self.index];
        self.index += 1;
        t
    }
    fn eat(&mut self, token: Token<'a>) -> bool {
        if self.peek() == Some(token) {
            self.bump();
            true
        } else {
            false
        }
    }
    fn error<T>(&mut self, message: &str) -> Option<T> {
        self.diagnostics.push(Diagnostic {
            span: self.span(),
            message: message.into(),
        });
        None
    }
    fn expect(&mut self, token: Token<'a>, message: &str) -> Option<()> {
        if self.eat(token) {
            Some(())
        } else {
            self.error(message)
        }
    }
    fn located<T>(&self, start: Pos, value: T) -> Loc<T> {
        Loc {
            value,
            span: Span {
                start,
                end: self.tokens[self.index - 1].span.end,
            },
        }
    }
    fn keyword(&self) -> bool {
        matches!(self.peek(), Some(Token::Identifier(_)))
            && self
                .tokens
                .get(self.index + 1)
                .is_some_and(|t| t.value == Token::Colon)
    }
    fn operator(&self) -> Option<(&'static str, u8)> {
        match self.peek()? {
            Token::Plus => Some(("+", 2)),
            Token::Minus => Some(("-", 2)),
            Token::Star => Some(("*", 3)),
            Token::Slash => Some(("/", 3)),
            _ => None,
        }
    }

    // Payload parsers always use ordinary mode. The caller controls precedence.
    fn selector<T>(
        &mut self,
        mut payload: impl FnMut(&mut Self) -> Option<T>,
    ) -> Option<Selector<T>> {
        if let Some((operator, _)) = self.operator() {
            self.bump();
            return Some(Selector::Operator {
                operator: operator.into(),
                value: Box::new(payload(self)?),
            });
        }
        if self.keyword() {
            let mut parts = Vec::new();
            while self.keyword() {
                let Token::Identifier(name) = self.bump().value else {
                    unreachable!()
                };
                self.bump();
                parts.push((name.into(), payload(self)?));
            }
            return Some(Selector::Keyword(parts));
        }
        if let Some(Token::Identifier(name)) = self.peek() {
            self.bump();
            Some(Selector::Atomic(name.into()))
        } else {
            self.error("expected selector")
        }
    }

    fn pattern(&mut self, selector_mode: bool) -> Option<Loc<Pattern>> {
        let start = self.span().start;
        let value = if self.eat(Token::OpenParen) {
            let pattern = self.pattern(false)?;
            self.expect(Token::CloseParen, "expected ')'")?;
            pattern.value
        } else if self.eat(Token::Hash) {
            Pattern::Selector(self.selector(|p| p.pattern(false))?)
        } else if self.eat(Token::Underscore) {
            Pattern::Discard
        } else if selector_mode
            && (matches!(self.peek(), Some(Token::Identifier(_))) || self.operator().is_some())
        {
            Pattern::Selector(self.selector(|p| p.pattern(false))?)
        } else if let Some(Token::Identifier(name)) = self.peek() {
            self.bump();
            Pattern::Variable(name.into())
        } else {
            return self.error("expected pattern");
        };
        Some(self.located(start, value))
    }

    fn primary(&mut self) -> Option<Loc<Expr>> {
        let start = self.span().start;
        let value = if self.eat(Token::OpenParen) {
            let expr = self.expr(0)?;
            self.expect(Token::CloseParen, "expected ')'")?;
            expr.value
        } else if self.eat(Token::Hash) {
            let precedence = self.operator().map_or(2, |(_, precedence)| precedence + 1);
            Expr::Selector(self.selector(|p| p.expr(precedence))?)
        } else if self.eat(Token::Let) {
            let pattern = self.pattern(false)?;
            self.expect(Token::Equals, "expected '='")?;
            let value = Box::new(self.expr(0)?);
            self.expect(Token::Dot, "expected '.'")?;
            let body = Box::new(self.expr(0)?);
            Expr::Let(Let {
                pattern,
                value,
                body,
            })
        } else if self.eat(Token::OpenCurly) {
            let mut methods = Vec::new();
            if self.peek() == Some(Token::Def) {
                loop {
                    let start = self.span().start;
                    self.expect(Token::Def, "expected 'def'")?;
                    let pattern = self.pattern(true)?;
                    self.expect(Token::FatArrow, "expected '=>'")?;
                    let body = Box::new(self.expr(0)?);
                    methods.push(self.located(start, Method { pattern, body }));
                    if !self.eat(Token::Dot) {
                        break;
                    }
                }
            }
            self.expect(Token::CloseCurly, "expected '}'")?;
            Expr::Actor(Actor { methods })
        } else if let Some(Token::Identifier(name)) = self.peek() {
            if self.keyword() {
                return self.error("expected expression");
            }
            self.bump();
            Expr::Variable(name.into())
        } else {
            return self.error("expected expression");
        };
        Some(self.located(start, value))
    }

    fn expr(&mut self, minimum: u8) -> Option<Loc<Expr>> {
        let mut callee = self.primary()?;
        loop {
            let start = self.span().start;
            let message = if self.keyword() && minimum <= 1 {
                let selector = self.selector(|p| p.expr(2))?;
                self.located(start, Expr::Selector(selector))
            } else if let Some((operator, precedence)) =
                self.operator().filter(|(_, prec)| *prec >= minimum)
            {
                self.bump();
                let value = Box::new(self.expr(precedence + 1)?);
                self.located(
                    start,
                    Expr::Selector(Selector::Operator {
                        operator: operator.into(),
                        value,
                    }),
                )
            } else if !self.keyword() && matches!(self.peek(), Some(Token::Identifier(_))) {
                let selector = self.selector(|_| unreachable!())?;
                self.located(start, Expr::Selector(selector))
            } else if self.peek() == Some(Token::OpenParen) {
                self.primary()?
            } else {
                break;
            };
            let span = Span {
                start: callee.span.start,
                end: message.span.end,
            };
            callee = Loc {
                value: Expr::Send {
                    callee: Box::new(callee),
                    message: Box::new(message),
                },
                span,
            };
        }
        Some(callee)
    }

    fn ty(&mut self, selector_mode: bool) -> Option<Loc<types::Type>> {
        use types::{ActorType, MethodType, Type};
        let start = self.span().start;
        let value = if self.eat(Token::OpenParen) {
            let ty = self.ty(false)?;
            self.expect(Token::CloseParen, "expected ')'")?;
            ty.value
        } else if self.eat(Token::Hash) {
            Type::Selector(self.selector(|p| p.ty(false).map(|t| t.value))?)
        } else if self.eat(Token::OpenCurly) {
            let mut methods = Vec::new();
            if self.peek() != Some(Token::CloseCurly) {
                loop {
                    let input = self.ty(true)?.value;
                    self.expect(Token::Arrow, "expected '->'")?;
                    let output = self.ty(false)?.value;
                    methods.push(MethodType {
                        parameters: Vec::new(),
                        input,
                        output,
                    });
                    if !self.eat(Token::Dot) {
                        break;
                    }
                }
            }
            self.expect(Token::CloseCurly, "expected '}'")?;
            let actor = ActorType { methods };
            if !actor.has_disjoint_inputs() {
                self.diagnostics.push(Diagnostic {
                    span: self.located(start, ()).span,
                    message: "actor type method inputs must be disjoint".into(),
                });
                return None;
            }
            Type::Actor(actor)
        } else if selector_mode {
            Type::Selector(self.selector(|p| p.ty(false).map(|t| t.value))?)
        } else if self.peek() == Some(Token::Identifier("any")) {
            self.bump();
            Type::Any
        } else if self.peek() == Some(Token::Identifier("never")) {
            self.bump();
            Type::Never
        } else {
            return self.error("expected type");
        };
        Some(self.located(start, value))
    }
}

/// Parse one expression and require end of input, reporting errors separately.
pub fn parse(lexer: Lexer<'_>, diagnostics: &mut Vec<Diagnostic>) -> Program {
    let (mut parser, invalid) = Parser::new(lexer, diagnostics);
    let expression = parser.expr(0);
    if expression.is_some() && parser.peek().is_some() {
        parser.error::<()>("expected end of input");
    }
    Program {
        expressions: if invalid {
            Vec::new()
        } else {
            expression.into_iter().collect()
        },
    }
}

/// Parse a type in ordinary mode; actor method inputs enter selector mode.
pub fn parse_type(source: &str, diagnostics: &mut Vec<Diagnostic>) -> Option<Loc<types::Type>> {
    let (mut parser, invalid) = Parser::new(Lexer::new(source), diagnostics);
    let ty = parser.ty(false)?;
    if parser.peek().is_some() {
        return parser.error("expected end of input");
    }
    if invalid { None } else { Some(ty) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expression(source: &str) -> Loc<Expr> {
        let mut diagnostics = Vec::new();
        let mut program = parse(Lexer::new(source), &mut diagnostics);
        assert!(diagnostics.is_empty(), "{source}: {diagnostics:?}");
        program.expressions.pop().unwrap()
    }

    fn shape(expr: &Expr) -> String {
        match expr {
            Expr::Variable(name) => name.clone(),
            Expr::Actor(_) => "{}".into(),
            Expr::Let(_) => "let".into(),
            Expr::Send { callee, message } => format!("({} {})", shape(callee), shape(message)),
            Expr::Selector(Selector::Atomic(name)) => format!("#{name}"),
            Expr::Selector(Selector::Operator { operator, value }) => {
                format!("#{operator}[{}]", shape(value))
            }
            Expr::Selector(Selector::Keyword(parts)) => format!(
                "#{}",
                parts
                    .iter()
                    .map(|(name, value)| format!("{name}:[{}]", shape(value)))
                    .collect::<Vec<_>>()
                    .join(" ")
            ),
        }
    }

    #[test]
    fn type_selector_mode_applies_to_builtin_names_too() {
        let mut diagnostics = Vec::new();
        let ty = parse_type("{ any -> {}. never -> {} }", &mut diagnostics).unwrap();
        assert!(diagnostics.is_empty());
        let types::Type::Actor(actor) = ty.value else {
            panic!()
        };
        assert_eq!(
            actor.methods[0].input,
            types::Type::Selector(Selector::Atomic("any".into()))
        );
        assert_eq!(
            actor.methods[1].input,
            types::Type::Selector(Selector::Atomic("never".into()))
        );
    }

    #[test]
    fn precedence_and_associativity() {
        for (source, expected) in [
            ("a b c", "((a #b) #c)"),
            ("a + b * c - d / e", "((a #+[(b #*[c])]) #-[(d #/[e])])"),
            ("a / b * c", "((a #/[b]) #*[c])"),
            (
                "a foo: b bar + c baz: d * e",
                "(a #foo:[((b #bar) #+[c])] baz:[(d #*[e])])",
            ),
            ("a (b + c) d", "((a (b #+[c])) #d)"),
            ("#foo: x + y bar: z", "#foo:[(x #+[y])] bar:[z]"),
            ("#+ x * y", "#+[(x #*[y])]"),
            ("a foo: (b bar: c)", "(a #foo:[(b #bar:[c])])"),
            (
                "#outer: (#inner: x) next: y",
                "#outer:[#inner:[x]] next:[y]",
            ),
            ("(#foo: x) bar", "(#foo:[x] #bar)"),
            ("#foo: x bar", "#foo:[(x #bar)]"),
            ("#foo", "#foo"),
        ] {
            assert_eq!(shape(&expression(source)), expected, "{source}");
        }
    }

    #[test]
    fn patterns_switch_modes_only_at_boundaries() {
        let expr =
            expression("{def foo => {}. def (x) => x. def put: x at: #slot: y => y. def + z => z}");
        let Expr::Actor(actor) = expr.value else {
            panic!()
        };
        assert_eq!(
            actor.methods[0].pattern.value,
            Pattern::Selector(Selector::Atomic("foo".into()))
        );
        assert_eq!(
            actor.methods[1].pattern.value,
            Pattern::Variable("x".into())
        );
        let Pattern::Selector(Selector::Keyword(parts)) = &actor.methods[2].pattern.value else {
            panic!()
        };
        assert_eq!(parts[0].1.value, Pattern::Variable("x".into()));
        assert!(matches!(
            parts[1].1.value,
            Pattern::Selector(Selector::Keyword(_))
        ));
        let Expr::Let(binding) = expression("let #put: x = #put: {}. x").value else {
            panic!()
        };
        assert!(matches!(binding.pattern.value, Pattern::Selector(_)));
    }

    #[test]
    fn sends_and_groups_retain_spans() {
        let expr = expression(" a + (b c) ");
        assert_eq!(expr.span.start.col, 2);
        assert_eq!(expr.span.end.col, 11);
        let Expr::Send { message, .. } = expr.value else {
            panic!()
        };
        assert_eq!(message.span.start.col, 4);
        let Expr::Selector(Selector::Operator { value, .. }) = message.value else {
            panic!()
        };
        assert_eq!(value.span.start.col, 6);
        assert_eq!(value.span.end.col, 11);
    }

    #[test]
    fn malformed_selectors_and_sends() {
        for source in [
            "#",
            "#:",
            "#+",
            "#foo:",
            "a +",
            "a foo:",
            "a foo: b bar:",
            "()",
            "(a",
            "a (",
            "let # = x. x",
            "{def foo: => x}",
            "{def # => x}",
            "#foo:: x",
            "a -> b",
        ] {
            let mut diagnostics = Vec::new();
            parse(Lexer::new(source), &mut diagnostics);
            assert!(!diagnostics.is_empty(), "{source}");
        }
    }

    #[test]
    fn type_modes_and_disjointness() {
        for source in [
            "any",
            "never",
            "{}",
            "#foo",
            "#+ any",
            "#put: {} at: #slot",
            "{foo -> {}. bar -> any}",
            "{put: any -> #ok}",
            "{(any) -> never}",
        ] {
            let mut diagnostics = Vec::new();
            assert!(
                parse_type(source, &mut diagnostics).is_some(),
                "{source}: {diagnostics:?}"
            );
            assert!(diagnostics.is_empty(), "{source}: {diagnostics:?}");
        }
        for source in [
            "foo",
            "#",
            "{foo => any}",
            "{foo -> bar}",
            "{foo -> any. foo -> {}}",
            "{put: any -> any. put: {} -> {}}",
            "any never",
        ] {
            let mut diagnostics = Vec::new();
            assert!(parse_type(source, &mut diagnostics).is_none(), "{source}");
            assert!(!diagnostics.is_empty(), "{source}");
        }
    }

    #[test]
    fn selector_punctuation_lexes_without_splitting_arrows() {
        let mut lexer = Lexer::new("#:()+-*/->=>");
        let tokens = lexer.by_ref().map(|t| t.value).collect::<Vec<_>>();
        assert_eq!(
            tokens,
            vec![
                Token::Hash,
                Token::Colon,
                Token::OpenParen,
                Token::CloseParen,
                Token::Plus,
                Token::Minus,
                Token::Star,
                Token::Slash,
                Token::Arrow,
                Token::FatArrow
            ]
        );
        assert!(lexer.diagnostics.is_empty());
    }
}
