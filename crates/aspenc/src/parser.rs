use super::*;

pub(crate) struct Parser<'a, 'd> {
    tokens: Vec<Loc<Token<'a>>>,
    index: usize,
    eof: Pos,
    pub(crate) diagnostics: &'d mut Vec<Diagnostic>,
}

impl<'a, 'd> Parser<'a, 'd> {
    pub(crate) fn new(mut lexer: Lexer<'a>, diagnostics: &'d mut Vec<Diagnostic>) -> (Self, bool) {
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

    pub(crate) fn peek(&self) -> Option<Token<'a>> {
        self.tokens.get(self.index).map(|t| t.value)
    }
    pub(crate) fn span(&self) -> Span {
        self.tokens.get(self.index).map_or(
            Span {
                start: self.eof,
                end: self.eof,
            },
            |t| t.span,
        )
    }
    pub(crate) fn bump(&mut self) -> Loc<Token<'a>> {
        let t = self.tokens[self.index];
        self.index += 1;
        t
    }
    pub(crate) fn eat(&mut self, token: Token<'a>) -> bool {
        if self.peek() == Some(token) {
            self.bump();
            true
        } else {
            false
        }
    }
    pub(crate) fn error<T>(&mut self, message: &str) -> Option<T> {
        self.diagnostics.push(Diagnostic {
            span: self.span(),
            message: message.into(),
        });
        None
    }
    pub(crate) fn expect(&mut self, token: Token<'a>, message: &str) -> Option<()> {
        if self.eat(token) {
            Some(())
        } else {
            self.error(message)
        }
    }
    pub(crate) fn located<T>(&self, start: Pos, value: T) -> Loc<T> {
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
        // Parenthesized prefixes are ambiguous until a following pattern is seen.
        // Speculate only over type-prefix forms, never over bare selectors.
        if !selector_mode
            && matches!(
                self.peek(),
                Some(Token::Identifier(_) | Token::OpenParen | Token::OpenCurly)
            )
        {
            let index = self.index;
            let diagnostics = self.diagnostics.len();
            if let Some(ty) = self.ty(false)
                && !self.keyword()
                && matches!(
                    self.peek(),
                    Some(Token::Identifier(_) | Token::Underscore | Token::OpenParen)
                )
            {
                let start = ty.span.start;
                let pattern = Box::new(self.pattern_atom(false)?);
                return Some(self.located(start, Pattern::Annotated { ty, pattern }));
            }
            self.index = index;
            self.diagnostics.truncate(diagnostics);
        }
        self.pattern_atom(selector_mode)
    }

    fn pattern_atom(&mut self, selector_mode: bool) -> Option<Loc<Pattern>> {
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
        } else if self.eat(Token::OpenCurly) {
            let mut methods = Vec::new();
            while self.peek() == Some(Token::Def) {
                let start = self.bump().span.start;
                let pattern = self.pattern(true)?;
                let reply = if self.eat(Token::Arrow) {
                    Some(self.ty(false)?)
                } else {
                    None
                };
                self.expect(Token::FatArrow, "expected '=>'")?;
                let mut body = Vec::new();
                while !matches!(self.peek(), None | Some(Token::Def | Token::CloseCurly)) {
                    body.push(self.statement()?);
                }
                methods.push(self.located(
                    start,
                    Method {
                        pattern,
                        reply,
                        body,
                    },
                ));
            }
            self.expect(Token::CloseCurly, "expected '}'")?;
            Expr::Actor(Actor { methods })
        } else if let Some(Token::String(source)) = self.peek() {
            self.bump();
            Expr::String(strings::decode(source).expect("lexer validates string escapes"))
        } else if let Some(Token::Float(value)) = self.peek() {
            self.bump();
            Expr::Float(value)
        } else if let Some(Token::Int(value)) = self.peek() {
            self.bump();
            Expr::Int(value)
        } else if self.eat(Token::Caret) {
            Expr::ReplyTo
        } else if let Some(Token::Identifier(name)) = self.peek() {
            if self.keyword() {
                return self.error("expected expression");
            }
            let mut name = name.to_owned();
            let mut end = self.bump().span.end;
            while self.peek() == Some(Token::Slash) && self.span().start == end {
                let slash = self.bump();
                if self.span().start != slash.span.end {
                    return self.error("qualified names require adjacent '/' and identifiers; division requires spaces on both sides");
                }
                let Some(Token::Identifier(part)) = self.peek() else {
                    return self.error("expected name after '/' (division requires spaces on both sides)");
                };
                name.push('/');
                name.push_str(part);
                end = self.bump().span.end;
            }
            Expr::Variable(name)
        } else {
            return self.error("expected expression");
        };
        Some(self.located(start, value))
    }

    pub(crate) fn statement(&mut self) -> Option<Loc<Stmt>> {
        let start = self.span().start;
        let value = if self.eat(Token::Let) {
            let pattern = self.pattern(false)?;
            self.expect(Token::Equals, "expected '='")?;
            let value = Box::new(self.expr(0)?);
            Stmt::Let(Let { pattern, value })
        } else {
            Stmt::Expr(self.expr(0)?)
        };
        self.expect(Token::Dot, "expected '.'")?;
        Some(self.located(start, value))
    }

    fn expr(&mut self, minimum: u8) -> Option<Loc<Expr>> {
        let reply_send = self.peek() == Some(Token::Caret);
        let mut callee = self.primary()?;
        // A direct caret send takes an ordinary expression, not a selector.
        // Keep bare `^` available at expression boundaries for lexical aliases.
        if reply_send
            && matches!(
                self.peek(),
                Some(
                    Token::Identifier(_)
                        | Token::Int(_)
                        | Token::Float(_)
                        | Token::String(_)
                        | Token::Hash
                        | Token::OpenParen
                        | Token::OpenCurly
                        | Token::Caret
                )
            )
        {
            let message = self.expr(minimum)?;
            let span = Span {
                start: callee.span.start,
                end: message.span.end,
            };
            return Some(Loc {
                value: Expr::Send {
                    callee: Box::new(callee),
                    message: Box::new(message),
                },
                span,
            });
        }
        if reply_send {
            return Some(callee);
        }
        loop {
            let start = self.span().start;
            let message = if self.keyword() && minimum <= 1 {
                let selector = self.selector(|p| p.expr(2))?;
                self.located(start, Expr::Selector(selector))
            } else if let Some((operator, precedence)) =
                self.operator().filter(|(_, prec)| *prec >= minimum)
            {
                if operator == "/" {
                    let slash = self.tokens[self.index];
                    let left_adjacent = self.tokens[self.index - 1].span.end == slash.span.start;
                    let right_adjacent = self.tokens.get(self.index + 1)
                        .is_some_and(|next| slash.span.end == next.span.start);
                    if left_adjacent || right_adjacent {
                        return self.error("division requires spaces on both sides of '/' (qualified names use adjacent identifiers)");
                    }
                }
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

    fn ty(&mut self, selector_mode: bool) -> Option<Loc<TypeExpr>> {
        let start = self.span().start;
        let value = if self.eat(Token::OpenParen) {
            let ty = self.ty(false)?;
            self.expect(Token::CloseParen, "expected ')'")?;
            ty.value
        } else if self.eat(Token::Hash) {
            TypeExpr::Selector(self.selector(|p| p.ty(false))?)
        } else if self.eat(Token::OpenCurly) {
            let mut methods = Vec::new();
            if self.peek() != Some(Token::CloseCurly) {
                loop {
                    let method_start = self.span().start;
                    let input = self.ty(true)?;
                    let reply = if self.eat(Token::Arrow) {
                        Some(self.ty(false)?)
                    } else {
                        None
                    };
                    methods.push(self.located(method_start, TypeMethod { input, reply }));
                    if !self.eat(Token::Dot) {
                        break;
                    }
                }
            }
            self.expect(Token::CloseCurly, "expected '}'")?;
            TypeExpr::Actor(methods)
        } else if selector_mode {
            TypeExpr::Selector(self.selector(|p| p.ty(false))?)
        } else if let Some(Token::Identifier(name)) = self.peek() {
            self.bump();
            match name {
                "never" => TypeExpr::Never,
                "bytes" => TypeExpr::Bytes,
                "string" => TypeExpr::String,
                "int" => TypeExpr::Int,
                "float" => TypeExpr::Float,
                "selector" => TypeExpr::SelectorFamily,
                "atom" => TypeExpr::Atom,
                "optagged" => TypeExpr::OpTagged,
                "keywordtagged" => TypeExpr::KeywordTagged,
                _ => TypeExpr::Variable(name.into()),
            }
        } else {
            return self.error("expected type");
        };
        Some(self.located(start, value))
    }
}

/// Parse zero or more period-terminated statements, reporting errors separately.
pub fn parse(lexer: Lexer<'_>, diagnostics: &mut Vec<Diagnostic>) -> Program {
    let (mut parser, invalid) = Parser::new(lexer, diagnostics);
    let mut statements = Vec::new();
    while parser.peek().is_some() {
        let Some(statement) = parser.statement() else {
            break;
        };
        statements.push(statement);
    }
    if invalid {
        statements.clear();
    }
    Program { statements }
}

/// Parse a type in ordinary mode; actor method inputs enter selector mode.
pub fn parse_type_expression(
    source: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Loc<TypeExpr>> {
    let (mut parser, invalid) = Parser::new(Lexer::new(source), diagnostics);
    let ty = parser.ty(false)?;
    if parser.peek().is_some() {
        return parser.error("expected end of input");
    }
    if invalid { None } else { Some(ty) }
}

/// Parse and resolve a concrete type in an empty type environment.
pub fn parse_type(source: &str, diagnostics: &mut Vec<Diagnostic>) -> Option<Loc<types::Type>> {
    let syntax = parse_type_expression(source, diagnostics)?;
    match types::resolve_type(&types::TypeEnvironment::default(), &syntax) {
        Ok(value) => Some(Loc {
            value,
            span: syntax.span,
        }),
        Err(error) => {
            let span = match &error {
                types::TypeError::UnknownType { span, .. }
                | types::TypeError::InvalidActorType { span } => *span,
                _ => syntax.span,
            };
            diagnostics.push(Diagnostic {
                span,
                message: error.to_string(),
            });
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expression(source: &str) -> Loc<Expr> {
        let mut diagnostics = Vec::new();
        let source = format!("{source}.");
        let mut program = parse(Lexer::new(&source), &mut diagnostics);
        assert!(diagnostics.is_empty(), "{source}: {diagnostics:?}");
        let Stmt::Expr(expr) = program.statements.pop().unwrap().value else {
            panic!()
        };
        expr
    }

    fn shape(expr: &Expr) -> String {
        match expr {
            Expr::Int(value) => value.to_string(),
            Expr::Float(value) => value.to_string(),
            Expr::String(value) => format!("{value:?}"),
            Expr::ReplyTo => "^".into(),
            Expr::Variable(name) => name.clone(),
            Expr::Actor(_) => "{}".into(),
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

    fn binding_pattern(source: &str) -> Loc<Pattern> {
        let mut diagnostics = Vec::new();
        let source = format!("{source}.");
        let program = parse(Lexer::new(&source), &mut diagnostics);
        assert!(diagnostics.is_empty(), "{source}: {diagnostics:?}");
        let Stmt::Let(binding) = program.statements.into_iter().next().unwrap().value else {
            panic!()
        };
        binding.pattern
    }

    #[test]
    fn annotations_preserve_type_and_pattern_locations() {
        let pattern = binding_pattern("let int abc = {}. abc");
        assert_eq!(pattern.span.start.col, 5);
        assert_eq!(pattern.span.end.col, 12);
        let Pattern::Annotated { ty, pattern } = pattern.value else {
            panic!()
        };
        assert_eq!(ty.value, TypeExpr::Int);
        assert_eq!(ty.span.start.col, 5);
        assert_eq!(ty.span.end.col, 8);
        assert_eq!(pattern.value, Pattern::Variable("abc".into()));
        assert_eq!(pattern.span.start.col, 9);
        assert_eq!(pattern.span.end.col, 12);
    }

    #[test]
    fn methods_preserve_explicit_reply_types_and_locations() {
        let Expr::Actor(actor) =
            expression("{def done -> #done => ^ (#done). def stop -> never => def notify =>}")
                .value
        else {
            panic!()
        };
        let method = &actor.methods[0];
        assert_eq!(method.span.start.col, 2);
        assert_eq!(method.span.end.col, 33);
        assert_eq!(method.pattern.span.start.col, 6);
        assert_eq!(method.pattern.span.end.col, 10);
        let reply = method.reply.as_ref().unwrap();
        assert_eq!(
            reply.value,
            TypeExpr::Selector(Selector::Atomic("done".into()))
        );
        assert_eq!(reply.span.start.col, 14);
        assert_eq!(reply.span.end.col, 19);
        let Stmt::Expr(send) = &method.body[0].value else {
            panic!()
        };
        assert_eq!(shape(send), "(^ #done)");
        let Expr::Send { callee, .. } = &send.value else {
            panic!()
        };
        assert_eq!(callee.value, Expr::ReplyTo);
        assert_eq!(callee.span.start.col, 23);
        assert_eq!(callee.span.end.col, 24);
        assert_eq!(
            actor.methods[1].reply.as_ref().unwrap().value,
            TypeExpr::Never
        );
        assert!(actor.methods[1].body.is_empty());
        assert_eq!(actor.methods[1].span.end.col, 54);
        assert!(actor.methods[2].reply.is_none());
        for source in [
            "{def put: int x -> int => ^ (x).}",
            "{def (int x) -> (#ok: int) =>}",
            "{def + x -> {done -> #ok} =>}",
            "{def _ -> Reply =>}",
        ] {
            expression(source);
        }
    }

    #[test]
    fn malformed_reply_annotations_are_rejected() {
        for (source, expected) in [
            ("{def done -> =>}.", "expected type"),
            ("{def done ->", "expected type"),
            ("{def done -> ^ =>}.", "expected type"),
            ("{def done -> # =>}.", "expected selector"),
            ("{def done -> int}.", "expected '=>'"),
            ("{def done -> int -> int =>}.", "expected '=>'"),
        ] {
            let mut diagnostics = Vec::new();
            parse(Lexer::new(source), &mut diagnostics);
            assert_eq!(diagnostics[0].message, expected, "{source}");
        }
    }

    #[test]
    fn reply_target_is_an_ordinary_expression_but_not_a_name_or_pattern() {
        for (source, expected) in [
            ("^", "^"),
            ("^ (#done)", "(^ #done)"),
            ("^ done", "(^ done)"),
            ("^ #done", "(^ #done)"),
            ("^ x foo", "(^ (x #foo))"),
            ("^ x + y", "(^ (x #+[y]))"),
            ("^ x put: y", "(^ (x #put:[y]))"),
            ("^ {}", "(^ {})"),
            ("(^) done", "(^ #done)"),
            ("target (^)", "(target ^)"),
            ("#target: ^", "#target:[^]"),
            ("^ #+ x", "(^ #+[x])"),
        ] {
            assert_eq!(shape(&expression(source)), expected);
        }
        for source in [
            "let ^ = {}.",
            "let int ^ = {}.",
            "{def ^ =>}.",
            "{def (^) =>}.",
            "{def put: ^ =>}.",
            "#^.",
            "^: x.",
            "^ + x.",
            "^ put: x.",
        ] {
            let mut diagnostics = Vec::new();
            parse(Lexer::new(source), &mut diagnostics);
            assert!(!diagnostics.is_empty(), "{source}");
        }
    }

    #[test]
    fn annotations_bind_more_tightly_than_selectors() {
        let pattern = binding_pattern("let #x: y z: int abc = {}. abc");
        let Pattern::Selector(Selector::Keyword(parts)) = pattern.value else {
            panic!()
        };
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].1.value, Pattern::Variable("y".into()));
        assert!(matches!(parts[1].1.value, Pattern::Annotated { .. }));
        for source in [
            "let int (#x: y) = {}. y",
            "let (#x: T) y = {}. y",
            "let {} x = {}. x",
            "let (int) _ = {}. {}",
            "let T x = {}. x",
            "let int (T x) = {}. x",
        ] {
            assert!(
                matches!(binding_pattern(source).value, Pattern::Annotated { .. }),
                "{source}"
            );
        }
        for source in ["let A B x = {}. x", "let int #x: y = {}. y"] {
            let mut diagnostics = Vec::new();
            parse(Lexer::new(source), &mut diagnostics);
            assert!(!diagnostics.is_empty(), "{source}");
        }
    }

    #[test]
    fn annotations_do_not_change_receiver_selector_mode() {
        let Expr::Actor(actor) =
            expression("{def x => {}. def (int x) => x. def put: T x => x.}").value
        else {
            panic!()
        };
        assert_eq!(
            actor.methods[0].pattern.value,
            Pattern::Selector(Selector::Atomic("x".into()))
        );
        assert!(matches!(
            actor.methods[1].pattern.value,
            Pattern::Annotated { .. }
        ));
        let Pattern::Selector(Selector::Keyword(parts)) = &actor.methods[2].pattern.value else {
            panic!()
        };
        assert!(matches!(parts[0].1.value, Pattern::Annotated { .. }));
    }

    #[test]
    fn type_syntax_preserves_unknown_names_and_method_locations() {
        let mut diagnostics = Vec::new();
        let syntax = parse_type_expression("{put: T -> U}", &mut diagnostics).unwrap();
        assert!(diagnostics.is_empty());
        let TypeExpr::Actor(methods) = syntax.value else {
            panic!()
        };
        assert_eq!(methods[0].span.start.col, 2);
        assert_eq!(methods[0].span.end.col, 13);
        assert_eq!(
            methods[0].reply.as_ref().unwrap().value,
            TypeExpr::Variable("U".into())
        );
        let TypeExpr::Selector(Selector::Keyword(parts)) = &methods[0].input.value else {
            panic!()
        };
        assert_eq!(parts[0].1.value, TypeExpr::Variable("T".into()));
        assert_eq!(parts[0].1.span.start.col, 7);
        assert_eq!(parts[0].1.span.end.col, 8);
        assert!(parse_type_expression("{foo -> int. foo -> int}", &mut diagnostics).is_some());
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn actor_type_signatures_distinguish_no_reply_from_never() {
        let mut diagnostics = Vec::new();
        let syntax = parse_type_expression(
            "{notify: int. stop -> never. read -> int}",
            &mut diagnostics,
        )
        .unwrap();
        assert!(diagnostics.is_empty());
        let TypeExpr::Actor(methods) = syntax.value else {
            panic!()
        };
        assert_eq!(methods.len(), 3);
        assert!(methods[0].reply.is_none());
        assert_eq!(methods[0].span.start.col, 2);
        assert_eq!(methods[0].span.end.col, 13);
        assert_eq!(methods[1].reply.as_ref().unwrap().value, TypeExpr::Never);
        assert_eq!(methods[2].reply.as_ref().unwrap().value, TypeExpr::Int);
        for source in ["{foo ->}", "{foo.}", "{foo bar}"] {
            let mut diagnostics = Vec::new();
            assert!(
                parse_type_expression(source, &mut diagnostics).is_none(),
                "{source}"
            );
            assert!(!diagnostics.is_empty());
        }
    }

    #[test]
    fn type_selector_mode_applies_to_builtin_names_too() {
        let mut diagnostics = Vec::new();
        let ty = parse_type("{ int -> {}. never -> {} }", &mut diagnostics).unwrap();
        assert!(diagnostics.is_empty());
        let types::Type::Actor(actor) = ty.value else {
            panic!()
        };
        assert_eq!(
            actor.methods[0].input,
            types::Type::Selector(Selector::Atomic("int".into()))
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
        let expr = expression(
            "{def foo => {}. def (x) => x. def put: x at: #slot: y => y. def + z => z.}",
        );
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
        assert!(matches!(
            binding_pattern("let #put: x = #put: {}. x").value,
            Pattern::Selector(_)
        ));
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
    fn primitive_type_names_resolve_in_ordinary_mode() {
        for (source, syntax, semantic) in [
            ("bytes", TypeExpr::Bytes, types::Type::Bytes),
            ("string", TypeExpr::String, types::Type::String),
            ("int", TypeExpr::Int, types::Type::Int),
            ("float", TypeExpr::Float, types::Type::Float),
            (
                "selector",
                TypeExpr::SelectorFamily,
                types::Type::SelectorFamily,
            ),
            ("atom", TypeExpr::Atom, types::Type::Atom),
            ("optagged", TypeExpr::OpTagged, types::Type::OpTagged),
            (
                "keywordtagged",
                TypeExpr::KeywordTagged,
                types::Type::KeywordTagged,
            ),
        ] {
            let mut diagnostics = Vec::new();
            assert_eq!(
                parse_type_expression(source, &mut diagnostics)
                    .unwrap()
                    .value,
                syntax
            );
            assert_eq!(
                parse_type(source, &mut diagnostics).unwrap().value,
                semantic
            );
            let actor = format!("{{ {source} -> {{}} }}");
            let types::Type::Actor(actor) = parse_type(&actor, &mut diagnostics).unwrap().value
            else {
                panic!()
            };
            assert_eq!(
                actor.methods[0].input,
                types::Type::Selector(Selector::Atomic(source.into()))
            );
            assert!(diagnostics.is_empty());
        }
    }

    #[test]
    fn any_is_an_ordinary_name_not_a_builtin_type() {
        let mut diagnostics = Vec::new();
        assert_eq!(
            parse_type_expression("any", &mut diagnostics)
                .unwrap()
                .value,
            TypeExpr::Variable("any".into())
        );
        assert!(diagnostics.is_empty());
        assert!(parse_type("any", &mut diagnostics).is_none());
        assert!(!diagnostics.is_empty());
        assert_eq!(expression("any").value, Expr::Variable("any".into()));
        assert_eq!(
            binding_pattern("let any = {}. any").value,
            Pattern::Variable("any".into())
        );
    }

    #[test]
    fn type_modes_and_disjointness() {
        for source in [
            "int",
            "never",
            "{}",
            "#foo",
            "#+ int",
            "#put: {} at: #slot",
            "{foo -> {}. bar -> int}",
            "{put: int -> #ok}",
            "{(int) -> never}",
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
            "{foo => int}",
            "{foo -> bar}",
            "{foo -> int. foo -> {}}",
            "{put: int -> int. put: {} -> {}}",
            "int never",
        ] {
            let mut diagnostics = Vec::new();
            assert!(parse_type(source, &mut diagnostics).is_none(), "{source}");
            assert!(!diagnostics.is_empty(), "{source}");
        }
    }

    #[test]
    fn selector_punctuation_lexes_without_splitting_arrows() {
        let mut lexer = Lexer::new("#:()+-*/->=>^");
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
                Token::FatArrow,
                Token::Caret
            ]
        );
        assert!(lexer.diagnostics.is_empty());
    }
}
