use crate::{Diagnostic, Lexer, Pos, Span, Token};

impl<'a> Lexer<'a> {
    pub(crate) fn string_token(&mut self, start: Pos) -> Option<Token<'a>> {
        self.advance('"');
        let source = self.remaining;
        let mut escaped = false;
        while let Some(ch) = self.remaining.chars().next() {
            if ch == '\n' || ch == '\r' {
                break;
            }
            if ch == '"' && !escaped {
                let contents = &source[..source.len() - self.remaining.len()];
                self.advance(ch);
                return match decode(contents) {
                    Ok(_) => Some(Token::String(contents)),
                    Err(message) => {
                        self.diagnostics.push(Diagnostic {
                            span: Span {
                                start,
                                end: self.pos,
                            },
                            message: message.into(),
                        });
                        None
                    }
                };
            }
            escaped = ch == '\\' && !escaped;
            self.advance(ch);
        }
        self.diagnostics.push(Diagnostic {
            span: Span {
                start,
                end: self.pos,
            },
            message: "unterminated string literal; raw newlines are not allowed".into(),
        });
        None
    }
}

/// Tokens borrow their validated spelling; the AST owns decoded UTF-8 text.
pub(crate) fn decode(source: &str) -> Result<String, &'static str> {
    let mut output = String::new();
    let mut chars = source.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            output.push(ch);
            continue;
        }
        output.push(match chars.next().ok_or("incomplete string escape")? {
            '"' => '"',
            '\\' => '\\',
            'n' => '\n',
            'r' => '\r',
            't' => '\t',
            '0' => '\0',
            'u' => {
                if chars.next() != Some('{') {
                    return Err("Unicode escape requires braces: \\u{...}");
                }
                let mut value = 0u32;
                let mut digits = 0;
                loop {
                    let ch = chars.next().ok_or("unterminated Unicode escape")?;
                    if ch == '}' {
                        break;
                    }
                    let digit = ch
                        .to_digit(16)
                        .ok_or("Unicode escape requires hexadecimal digits")?;
                    digits += 1;
                    if digits > 6 {
                        return Err("Unicode escape requires one to six hexadecimal digits");
                    }
                    value = value * 16 + digit;
                }
                if digits == 0 {
                    return Err("Unicode escape requires one to six hexadecimal digits");
                }
                char::from_u32(value).ok_or("Unicode escape is not a Unicode scalar value")?
            }
            _ => return Err("unknown string escape"),
        });
    }
    Ok(output)
}
