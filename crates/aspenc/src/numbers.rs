use std::fmt;

use crate::{Diagnostic, Lexer, Pos, Span, Token};

/// A finite binary64 value. Structural equality preserves the sign of zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FloatValue(u64);

impl FloatValue {
    pub fn new(value: f64) -> Option<Self> {
        value.is_finite().then(|| Self(value.to_bits()))
    }

    pub fn get(self) -> f64 {
        f64::from_bits(self.0)
    }
}

impl fmt::Display for FloatValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Erlang requires a decimal point even for integral mantissas.
        let scientific = format!("{:e}", self.get());
        let (mantissa, exponent) = scientific.split_once('e').unwrap();
        write!(
            f,
            "{mantissa}{}e{exponent}",
            if mantissa.contains('.') { "" } else { ".0" }
        )
    }
}

impl<'a> Lexer<'a> {
    fn numeric_digits(&mut self) {
        while let Some(ch) = self
            .remaining
            .chars()
            .next()
            .filter(|ch| ch.is_ascii_digit() || *ch == '_')
        {
            self.advance(ch);
        }
    }

    pub(crate) fn number_token(&mut self, start: Pos) -> Option<Token<'a>> {
        let source = self.remaining;
        if self.remaining.starts_with('-') {
            self.advance('-');
        }
        self.numeric_digits();
        let mut float = false;
        // Leave statement terminators alone: a fraction starts with dot + digit.
        if self.remaining.starts_with('.')
            && self
                .remaining
                .as_bytes()
                .get(1)
                .is_some_and(u8::is_ascii_digit)
        {
            float = true;
            self.advance('.');
            self.numeric_digits();
        }
        if self.remaining.starts_with(['e', 'E']) {
            float = true;
            self.advance(self.remaining.chars().next().unwrap());
            if self.remaining.starts_with(['+', '-']) {
                self.advance(self.remaining.chars().next().unwrap());
            }
            self.numeric_digits();
        }
        let spelling = &source[..source.len() - self.remaining.len()];
        let span = Span {
            start,
            end: self.pos,
        };
        let bytes = spelling.as_bytes();
        if bytes.iter().enumerate().any(|(i, byte)| {
            *byte == b'_'
                && (i == 0
                    || !bytes[i - 1].is_ascii_digit()
                    || !bytes.get(i + 1).is_some_and(u8::is_ascii_digit))
        }) {
            self.diagnostics.push(Diagnostic {
                span,
                message: format!(
                    "{} separators must occur between digits",
                    if float { "float" } else { "integer" }
                ),
            });
            return None;
        }
        let normalized = spelling.replace('_', "");
        if float {
            match normalized.parse::<f64>() {
                Ok(value) => match FloatValue::new(value) {
                    Some(value) => Some(Token::Float(value)),
                    None => {
                        self.diagnostics.push(Diagnostic {
                            span,
                            message: "float literal is outside the finite binary64 range".into(),
                        });
                        None
                    }
                },
                Err(_) => {
                    self.diagnostics.push(Diagnostic {
                        span,
                        message: "float exponent requires decimal digits".into(),
                    });
                    None
                }
            }
        } else {
            // Parse the sign with the magnitude so i64::MIN remains accepted.
            match normalized.parse::<i64>() {
                Ok(value) => Some(Token::Int(value)),
                Err(_) => {
                    self.diagnostics.push(Diagnostic {
                        span,
                        message: "integer literal is outside the signed 64-bit range".into(),
                    });
                    None
                }
            }
        }
    }
}
