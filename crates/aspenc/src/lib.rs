//! The first building blocks of the Aspen compiler.

use std::ops::{Deref, DerefMut};

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
            } else {
                self.advance(ch);
                match ch {
                    '{' => Token::OpenCurly,
                    '}' => Token::CloseCurly,
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
pub struct Actor {}

#[derive(Debug, PartialEq, Eq)]
pub enum Expr {
    Actor(Actor),
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

            #[no_eof]
            pub rule expr() -> Option<Loc<Expr>>
                = whitespace() open:$([Loc { value: Token::OpenCurly, .. }]) whitespace()
                  close:$([Loc { value: Token::CloseCurly, .. }]) {
                    Some(Loc {
                        value: Expr::Actor(Actor {}),
                        span: Span { start: open[0].span.start, end: close[0].span.end },
                    })
                  }
                / whitespace() [Loc { value: Token::OpenCurly, .. }] whitespace() span:here() {
                    diagnostics.push(Diagnostic { span, message: "expected '}'".into() });
                    None
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
    // Skipping invalid characters must not turn malformed source into a valid actor.
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
                value: Expr::Actor(Actor {}),
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
        for source in ["{x}", "x{}", "{}x", "é", "}", "{{}"] {
            let (program, diagnostics) = parsed(source);
            assert!(program.expressions.is_empty(), "{source}");
            assert!(!diagnostics.is_empty(), "{source}");
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
        for mut n in 0..1024 {
            let mut source = String::new();
            for _ in 0..5 {
                source.push(['{', '}', ' ', 'x'][n % 4]);
                n /= 4;
            }
            parsed(&source);
        }
    }
}
