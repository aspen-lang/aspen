//! The first building blocks of the Aspen compiler.

use std::ops::{Deref, DerefMut};

pub mod types;

/// One-based line and Unicode scalar column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pos {
    pub line: u16,
    pub col: u16,
}

/// A half-open source range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: Pos,
    pub end: Pos,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Loc<T> {
    pub value: T,
    pub span: Span,
}

impl<T> Deref for Loc<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.value
    }
}

impl<T> DerefMut for Loc<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.value
    }
}

impl<T> AsRef<T> for Loc<T> {
    fn as_ref(&self) -> &T {
        &self.value
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Token<'a> {
    Whitespace(&'a str),
    OpenCurly,
    CloseCurly,
    Let,
    Def,
    FatArrow,
    Identifier(&'a str),
    Underscore,
    Equals,
    Dot,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub span: Span,
    pub message: String,
}

pub struct Lexer<'a> {
    remaining: &'a str,
    pos: Pos,
    previous_cr: bool,
    overflowed: bool,
    pub diagnostics: Vec<Diagnostic>,
}

impl<'a> Lexer<'a> {
    pub fn new(source: &'a str) -> Self {
        Self {
            remaining: source,
            pos: Pos { line: 1, col: 1 },
            previous_cr: false,
            overflowed: false,
            diagnostics: Vec::new(),
        }
    }

    pub fn position(&self) -> Pos {
        self.pos
    }

    fn advance(&mut self, ch: char) {
        self.remaining = &self.remaining[ch.len_utf8()..];
        let next = if ch == '\n' && self.previous_cr {
            Some(self.pos)
        } else if ch == '\n' || ch == '\r' {
            self.pos
                .line
                .checked_add(1)
                .map(|line| Pos { line, col: 1 })
        } else {
            self.pos
                .col
                .checked_add(1)
                .map(|col| Pos { col, ..self.pos })
        };
        self.previous_cr = ch == '\r';
        if let Some(pos) = next {
            self.pos = pos;
        } else if !self.overflowed {
            self.overflowed = true;
            self.diagnostics.push(Diagnostic {
                span: Span {
                    start: self.pos,
                    end: self.pos,
                },
                message: "source position exceeds u16 capacity".into(),
            });
        }
    }
}

impl<'a> Iterator for Lexer<'a> {
    type Item = Loc<Token<'a>>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let ch = self.remaining.chars().next()?;
            let start = self.pos;
            let token = if ch.is_whitespace() {
                let source = self.remaining;
                while let Some(ch) = self
                    .remaining
                    .chars()
                    .next()
                    .filter(|ch| ch.is_whitespace())
                {
                    self.advance(ch);
                }
                Token::Whitespace(&source[..source.len() - self.remaining.len()])
            } else if ch.is_alphabetic() || ch == '_' {
                let source = self.remaining;
                while let Some(ch) = self
                    .remaining
                    .chars()
                    .next()
                    .filter(|ch| ch.is_alphanumeric() || *ch == '_')
                {
                    self.advance(ch);
                }
                match &source[..source.len() - self.remaining.len()] {
                    "let" => Token::Let,
                    "def" => Token::Def,
                    "_" => Token::Underscore,
                    name => Token::Identifier(name),
                }
            } else if self.remaining.starts_with("=>") {
                self.advance('=');
                self.advance('>');
                Token::FatArrow
            } else {
                self.advance(ch);
                match ch {
                    '{' => Token::OpenCurly,
                    '}' => Token::CloseCurly,
                    '_' => Token::Underscore,
                    '=' => Token::Equals,
                    '.' => Token::Dot,
                    _ => {
                        self.diagnostics.push(Diagnostic {
                            span: Span {
                                start,
                                end: self.pos,
                            },
                            message: format!("unexpected character {ch:?}"),
                        });
                        continue;
                    }
                }
            };
            return Some(Loc {
                value: token,
                span: Span {
                    start,
                    end: self.pos,
                },
            });
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Actor {
    pub methods: Vec<Loc<Method>>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Method {
    pub pattern: Loc<Pattern>,
    pub body: Box<Loc<Expr>>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Pattern {
    Discard,
    Variable(String),
}

#[derive(Debug, PartialEq, Eq)]
pub struct Let {
    pub pattern: Loc<Pattern>,
    pub value: Box<Loc<Expr>>,
    pub body: Box<Loc<Expr>>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Expr {
    Actor(Actor),
    Variable(String),
    Let(Let),
}

#[derive(Debug, PartialEq, Eq)]
pub struct Program {
    pub expressions: Vec<Loc<Expr>>,
}

// Generated helper rules share the growable diagnostic sink.
#[allow(clippy::ptr_arg)]
mod parser {
    use super::*;
    peg::parser! {
        pub grammar grammar<'a>(eof: Pos, diagnostics: &mut Vec<Diagnostic>) for [Loc<Token<'a>>] {
            rule whitespace() = [Loc { value: Token::Whitespace(_), .. }]*

            rule here() -> Span
                = t:$([_]) { t[0].span }
                / ![_] { Span { start: eof, end: eof } }

            rule pattern() -> Loc<Pattern>
                = whitespace() token:$([Loc { value: Token::Underscore, .. }]) {
                    Loc { value: Pattern::Discard, span: token[0].span }
                  }
                / whitespace() token:$([Loc { value: Token::Identifier(_), .. }]) {
                    let Token::Identifier(name) = token[0].value else { unreachable!() };
                    Loc { value: Pattern::Variable(name.into()), span: token[0].span }
                  }

            rule let_body() -> Option<Loc<Expr>>
                = whitespace() [Loc { value: Token::Dot, .. }] body:expr() { body }
                / whitespace() span:here() {
                    diagnostics.push(Diagnostic { span, message: "expected '.'".into() });
                    None
                  }

            rule let_value() -> Option<(Loc<Expr>, Loc<Expr>)>
                = whitespace() [Loc { value: Token::Equals, .. }] value:expr() body:let_body() {
                    value.zip(body)
                  }
                / whitespace() span:here() {
                    diagnostics.push(Diagnostic { span, message: "expected '='".into() });
                    None
                  }

            rule let_tail() -> Option<Let>
                = pattern:pattern() parts:let_value() {
                    parts.map(|(value, body)| Let {
                        pattern, value: Box::new(value), body: Box::new(body),
                    })
                  }
                / whitespace() span:here() {
                    diagnostics.push(Diagnostic { span, message: "expected pattern".into() });
                    None
                  }

            rule close_actor() -> Option<Span>
                = whitespace() close:$([Loc { value: Token::CloseCurly, .. }]) { Some(close[0].span) }
                / whitespace() span:here() {
                    diagnostics.push(Diagnostic { span, message: "expected '}'".into() }); None
                  }

            rule method_pattern() -> Option<Loc<Pattern>>
                = p:pattern() { Some(p) }
                / whitespace() span:here() {
                    diagnostics.push(Diagnostic { span, message: "expected pattern".into() }); None
                  }

            rule method_body() -> Option<Loc<Expr>>
                = whitespace() [Loc { value: Token::FatArrow, .. }] body:expr() { body }
                / whitespace() span:here() {
                    diagnostics.push(Diagnostic { span, message: "expected '=>'".into() }); None
                  }

            rule method() -> Option<Loc<Method>>
                = whitespace() keyword:$([Loc { value: Token::Def, .. }])
                  pattern:method_pattern() body:method_body() {
                    pattern.zip(body).map(|(pattern, body)| Loc {
                        span: Span { start: keyword[0].span.start, end: body.span.end },
                        value: Method { pattern, body: Box::new(body) },
                    })
                  }

            rule methods() -> Option<Vec<Loc<Method>>>
                = first:method() rest:(whitespace() [Loc { value: Token::Dot, .. }] m:method() { m })* {
                    std::iter::once(first).chain(rest).collect()
                  }
                / { Some(Vec::new()) }

            #[no_eof]
            pub rule expr() -> Option<Loc<Expr>>
                = whitespace() keyword:$([Loc { value: Token::Let, .. }]) binding:let_tail() {
                    binding.map(|binding| Loc {
                        span: Span { start: keyword[0].span.start, end: binding.body.span.end },
                        value: Expr::Let(binding),
                    })
                  }
                / whitespace() token:$([Loc { value: Token::Identifier(_), .. }]) {
                    let Token::Identifier(name) = token[0].value else { unreachable!() };
                    Some(Loc { value: Expr::Variable(name.into()), span: token[0].span })
                  }
                / whitespace() open:$([Loc { value: Token::OpenCurly, .. }]) methods:methods()
                  close:close_actor() {
                    match (methods, close) {
                        (Some(methods), Some(close)) => Some(Loc {
                            value: Expr::Actor(Actor { methods }),
                            span: Span { start: open[0].span.start, end: close.end },
                        }),
                        _ => None,
                    }
                  }
                / whitespace() span:here() {
                    diagnostics.push(Diagnostic { span, message: "expected expression".into() });
                    None
                  }

            pub rule program() -> Program
                = expression:expr() whitespace() trailing:$([_]*) ![_] {
                    if expression.is_some() && !trailing.is_empty() {
                        diagnostics.push(Diagnostic {
                            span: trailing[0].span,
                            message: "expected end of input".into(),
                        });
                    }
                    Program { expressions: expression.into_iter().collect() }
                  }
        }
    }
}
/// Parse one expression and require end of input, reporting errors separately.
pub fn parse(lexer: Lexer<'_>, diagnostics: &mut Vec<Diagnostic>) -> Program {
    let mut lexer = lexer;
    let tokens: Vec<_> = lexer.by_ref().collect();
    let invalid_source = !lexer.diagnostics.is_empty();
    let eof = lexer.position();
    diagnostics.append(&mut lexer.diagnostics);
    // Every grammar branch recovers, and program consumes all remaining tokens.
    let mut program = parser::grammar::program(&tokens, eof, diagnostics)
        .expect("program grammar must be infallible");
    // Skipping invalid characters must not turn malformed source into a valid expression.
    if invalid_source {
        program.expressions.clear();
    }
    program
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(source: &str) -> (Program, Vec<Diagnostic>) {
        let mut diagnostics = Vec::new();
        let program = parse(Lexer::new(source), &mut diagnostics);
        (program, diagnostics)
    }

    #[test]
    fn location_layout_and_access() {
        assert_eq!(size_of::<Pos>(), 4);
        assert_eq!(size_of::<Span>(), 8);
        let pos = Pos { line: 1, col: 1 };
        let mut loc = Loc {
            value: 1,
            span: Span {
                start: pos,
                end: pos,
            },
        };
        *loc = 2;
        assert_eq!(loc.as_ref(), &2);
    }

    #[test]
    fn whitespace_borrows_source_and_tracks_crlf_and_unicode() {
        let source = " \r\n\u{2003}{}";
        let mut lexer = Lexer::new(source);
        let whitespace = lexer.next().unwrap();
        assert_eq!(whitespace.value, Token::Whitespace(&source[..6]));
        if let Token::Whitespace(text) = whitespace.value {
            assert_eq!(text.as_ptr(), source.as_ptr());
        }
        assert_eq!(whitespace.span.end, Pos { line: 2, col: 2 });
        assert_eq!(lexer.next().unwrap().value, Token::OpenCurly);
        assert_eq!(lexer.next().unwrap().value, Token::CloseCurly);
        assert!(lexer.next().is_none());
        assert!(lexer.next().is_none());
    }

    #[test]
    fn actor_span_excludes_surrounding_whitespace() {
        let (program, diagnostics) = parsed(" \n{ \r\n }\t");
        assert!(diagnostics.is_empty());
        assert_eq!(
            program.expressions,
            vec![Loc {
                value: Expr::Actor(Actor {
                    methods: Vec::new()
                }),
                span: Span {
                    start: Pos { line: 2, col: 1 },
                    end: Pos { line: 3, col: 3 }
                },
            }]
        );
    }

    #[test]
    fn missing_expression_and_close_report_actual_eof() {
        for (source, message, eof) in [
            ("", "expected expression", Pos { line: 1, col: 1 }),
            (" \r\n", "expected expression", Pos { line: 2, col: 1 }),
            ("{ \n", "expected '}'", Pos { line: 2, col: 1 }),
        ] {
            let (program, diagnostics) = parsed(source);
            assert!(program.expressions.is_empty());
            assert_eq!(
                diagnostics,
                vec![Diagnostic {
                    span: Span {
                        start: eof,
                        end: eof
                    },
                    message: message.into(),
                }]
            );
        }
    }

    #[test]
    fn expr_does_not_require_eof_but_program_does() {
        let mut lexer = Lexer::new("{} {}");
        let tokens: Vec<_> = lexer.by_ref().collect();
        let mut diagnostics = Vec::new();
        assert!(
            parser::grammar::expr(&tokens, lexer.position(), &mut diagnostics)
                .unwrap()
                .is_some()
        );
        assert!(diagnostics.is_empty());
        let (program, diagnostics) = parsed("{} {}");
        assert_eq!(program.expressions.len(), 1);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].message, "expected end of input");
        assert_eq!(diagnostics[0].span.start.col, 4);
    }

    #[test]
    fn invalid_characters_never_create_valid_actors() {
        for source in ["{x}", "@{}", "{}@", "@", "}", "{{}"] {
            let (program, diagnostics) = parsed(source);
            assert!(program.expressions.is_empty(), "{source}");
            assert!(!diagnostics.is_empty(), "{source}");
        }
    }

    #[test]
    fn let_tokens_and_keyword_boundaries() {
        let mut lexer = Lexer::new("let_={}.{}");
        // An identifier-like continuation must not be split into the keyword.
        assert!(!lexer.by_ref().any(|token| token.value == Token::Let));
        assert!(lexer.diagnostics.is_empty());
        for source in ["letter", "let1", "leté"] {
            assert!(!Lexer::new(source).any(|token| token.value == Token::Let));
        }
        let mut lexer = Lexer::new("let _={}.{}");
        let tokens: Vec<_> = lexer.by_ref().collect();
        assert!(lexer.diagnostics.is_empty());
        assert_eq!(
            tokens.iter().map(|t| t.value).collect::<Vec<_>>(),
            vec![
                Token::Let,
                Token::Whitespace(" "),
                Token::Underscore,
                Token::Equals,
                Token::OpenCurly,
                Token::CloseCurly,
                Token::Dot,
                Token::OpenCurly,
                Token::CloseCurly,
            ]
        );
        assert_eq!(tokens[0].span.start.col, 1);
        assert_eq!(tokens[0].span.end.col, 4);
    }

    #[test]
    fn let_expression_has_located_pattern_value_and_body() {
        let (program, diagnostics) = parsed(" let _ = {} . {} ");
        assert!(diagnostics.is_empty());
        let expression = &program.expressions[0];
        assert_eq!(expression.span.start.col, 2);
        assert_eq!(expression.span.end.col, 17);
        let Expr::Let(binding) = &expression.value else {
            panic!("expected let")
        };
        assert_eq!(binding.pattern.value, Pattern::Discard);
        assert_eq!(binding.pattern.span.start.col, 6);
        assert_eq!(binding.pattern.span.end.col, 7);
        assert_eq!(
            binding.value.value,
            Expr::Actor(Actor {
                methods: Vec::new()
            })
        );
        assert_eq!(binding.value.span.start.col, 10);
        assert_eq!(binding.value.span.end.col, 12);
        assert_eq!(
            binding.body.value,
            Expr::Actor(Actor {
                methods: Vec::new()
            })
        );
        assert_eq!(binding.body.span.start.col, 15);
        assert_eq!(binding.body.span.end.col, 17);
    }

    #[test]
    fn actor_methods_parse_with_nested_lets_and_locations() {
        let (program, diagnostics) = parsed("{ def x => let y = x. y. def _ => {} }");
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let Expr::Actor(actor) = &program.expressions[0].value else {
            panic!()
        };
        assert_eq!(actor.methods.len(), 2);
        assert_eq!(actor.methods[0].span.start.col, 3);
        assert_eq!(actor.methods[0].span.end.col, 24);
        assert_eq!(actor.methods[0].pattern.span.start.col, 7);
        assert!(matches!(actor.methods[0].body.value, Expr::Let(_)));
        assert_eq!(actor.methods[1].pattern.value, Pattern::Discard);
        let (program, diagnostics) = parsed("{def _=>{def x=>x}}");
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(program.expressions.len(), 1);
    }

    #[test]
    fn malformed_methods_are_rejected() {
        for source in [
            "{ def }",
            "{ def x {} }",
            "{ def _ => }",
            "{ def x => x def y => y }",
            "{ def x => x. }",
            "def x => x",
            "{ x => x }",
            "{ def def => {} }",
            "{ def _ => {}",
        ] {
            let (_, diagnostics) = parsed(source);
            assert!(!diagnostics.is_empty(), "{source}");
        }
        let tokens: Vec<_> = Lexer::new("def defx => = >")
            .map(|token| token.value)
            .collect();
        assert_eq!(tokens[0], Token::Def);
        assert_eq!(tokens[2], Token::Identifier("defx"));
        assert_eq!(tokens[4], Token::FatArrow);
        assert_eq!(tokens[6], Token::Equals);
    }

    #[test]
    fn variable_patterns_and_reserved_words() {
        for name in ["x", "value1", "_value", "let_value", "letter", "é"] {
            let source = format!("let {name} = {{}}. {{}}");
            let (program, diagnostics) = parsed(&source);
            assert!(diagnostics.is_empty(), "{source}: {diagnostics:?}");
            let Expr::Let(binding) = &program.expressions[0].value else {
                panic!()
            };
            assert_eq!(binding.pattern.value, Pattern::Variable(name.into()));
        }
        for source in ["let let = {}. {}", "let 1x = {}. {}", "let x = {}. _"] {
            let (program, diagnostics) = parsed(source);
            assert!(program.expressions.is_empty(), "{source}");
            assert!(!diagnostics.is_empty(), "{source}");
        }
        let (_, diagnostics) = parsed("{}x");
        assert_eq!(diagnostics[0].message, "expected end of input");
    }

    #[test]
    fn lets_nest_in_value_and_body() {
        let (program, diagnostics) = parsed("let _ = let _ = {} . {} . let _ = {} . {}");
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let Expr::Let(binding) = &program.expressions[0].value else {
            panic!("expected let")
        };
        assert!(matches!(binding.value.value, Expr::Let(_)));
        assert!(matches!(binding.body.value, Expr::Let(_)));
    }

    #[test]
    fn malformed_lets_report_missing_parts() {
        for (source, message, col) in [
            ("let", "expected pattern", 4),
            ("let {}", "expected pattern", 5),
            ("let _", "expected '='", 6),
            ("let _ = . {}", "expected expression", 9),
            ("let _ = {}", "expected '.'", 11),
            ("let _ = {} .", "expected expression", 13),
        ] {
            let (program, diagnostics) = parsed(source);
            assert!(program.expressions.is_empty(), "{source}");
            assert_eq!(diagnostics[0].message, message, "{source}");
            assert_eq!(diagnostics[0].span.start.col, col, "{source}");
        }
    }

    #[test]
    fn position_overflow_is_reported_without_wrapping() {
        for source in [
            " ".repeat(u16::MAX as usize),
            "\n".repeat(u16::MAX as usize),
        ] {
            let mut lexer = Lexer::new(&source);
            lexer.by_ref().for_each(drop);
            assert_eq!(lexer.diagnostics.len(), 1);
            assert!(lexer.position().line > 0);
            assert!(lexer.position().col > 0);
        }
    }

    #[test]
    fn grammar_recovers_for_all_short_token_sequences() {
        for mut n in 0..10usize.pow(5) {
            let mut source = String::new();
            for _ in 0..5 {
                source.push_str(["{", "}", " ", "x", "let ", "_", "=", ".", "def ", "=>"][n % 10]);
                n /= 10;
            }
            parsed(&source);
        }
    }
}
