use crate::source::Source;
use crate::syntax::{Token, TokenKind, TokenKind::*};
use crate::Graphemes;
use peekmore::{PeekMore, PeekMoreIterator};
use std::sync::Arc;

pub struct Lexer<'a> {
    source: &'a Arc<Source>,
    chars: PeekMoreIterator<Graphemes<'a>>,
}

impl<'a> Lexer<'a> {
    pub fn tokenize(source: &Arc<Source>) -> Arc<Vec<Arc<Token>>> {
        let chars = source.graphemes().peekmore();
        let lexer = Lexer {
            source: &source,
            chars,
        };
        lexer.get_tokens()
    }

    fn get_tokens(mut self) -> Arc<Vec<Arc<Token>>> {
        let mut tokens = vec![];
        while let Some(_) = self.chars.peek() {
            tokens.push(self.next_token());
        }
        let end_offset = self.offset();
        tokens.push(Token::new(EOF, &self.source, end_offset..end_offset));
        Arc::new(tokens)
    }

    fn offset(&mut self) -> usize {
        self.chars
            .peek()
            .map(|(i, _)| *i)
            .unwrap_or_else(|| self.source.len())
    }

    fn peek(&mut self) -> &str {
        self.chars.peek().map(|(_, c)| *c).unwrap_or("")
    }

    fn peek_next(&mut self) -> &str {
        let result = self.chars.peek_next().map(|(_, c)| *c).unwrap_or("");
        self.chars.reset_view();
        result
    }

    fn peek_char(&mut self) -> char {
        self.peek().chars().next().unwrap_or(0 as char)
    }

    fn peek_next_char(&mut self) -> char {
        self.peek_next().chars().next().unwrap_or(0 as char)
    }

    fn take(&mut self) -> &str {
        self.chars.next().map(|(_, c)| c).unwrap_or("")
    }

    fn skip(&mut self) {
        self.chars.next().map(|_| ()).unwrap_or(());
    }

    fn next_token(&mut self) -> Arc<Token> {
        let start_offset = self.offset();
        let kind: TokenKind;

        match self.peek_char() {
            '.' => {
                self.skip();
                kind = Period;
            }

            '{' => {
                self.skip();
                kind = OpenCurly;
            }

            '}' => {
                self.skip();
                kind = CloseCurly;
            }

            c if c == '\n' => {
                self.skip();
                kind = Whitespace;
            }

            c if c.is_whitespace() => {
                self.skip_whitespace();
                kind = Whitespace;
            }

            c if c.is_numeric() || (c == '-' && self.peek_next_char().is_numeric()) => {
                kind = self.take_number();
            }

            c if c.is_alphabetic() => {
                kind = self.take_symbol_or_keyword();
            }

            _ => {
                self.skip();
                kind = Unknown;
            }
        }

        let end_offset = self.offset();

        if start_offset == end_offset {
            panic!("next_token didn't move forward");
        }

        Token::new(kind, &self.source, start_offset..end_offset)
    }

    fn take_symbol_or_keyword(&mut self) -> TokenKind {
        let symbol = self.take_symbol();

        let mut kind = match symbol {
            "object" => ObjectKeyword,
            _ => Identifier,
        };

        if let '!' | '?' = self.peek_char() {
            self.skip();
            kind = NullaryAtom;
        }

        kind
    }

    fn take_symbol(&mut self) -> &str {
        let start = self.peek().as_ptr();
        let mut length = 0;

        while self.peek_char().is_alphanumeric() {
            length += self.take().len();
        }

        while self.peek_char() == '\'' {
            length += self.take().len();
        }

        unsafe { std::str::from_utf8(std::slice::from_raw_parts(start, length)).unwrap() }
    }

    fn skip_whitespace(&mut self) {
        loop {
            let c = self.peek_char();

            if c.is_whitespace() && c != '\n' {
                self.skip();
                continue;
            }

            break;
        }
    }

    fn take_number(&mut self) -> TokenKind {
        let mut positive = true;
        if self.peek_char() == '-' {
            self.skip();
            positive = false;
        }

        let mut radix_or_integer = String::new();
        while self.peek_char().is_numeric() {
            radix_or_integer.push_str(self.take());
        }

        let mut radix = 10u32;
        let mut number;
        if self.peek_char() == '#' {
            self.skip();
            radix = match radix_or_integer.parse() {
                Ok(n) if n >= 2 && n <= 36 => n,
                _ => radix,
            };
            number = self.take_digits(radix);
        } else {
            number = radix_or_integer;
        }

        if !positive {
            number.insert(0, '-');
        }

        if self.peek_char() != '.' {
            return match i128::from_str_radix(&number, radix) {
                Ok(n) => TokenKind::IntegerLiteral(n, true),
                Err(_) => TokenKind::IntegerLiteral(0, false),
            };
        }
        self.take();
        let fraction = self.take_digits(radix);
        let precision = fraction.len();
        number.push_str(fraction.as_str());

        return match i64::from_str_radix(&number, radix) {
            Ok(n) => {
                TokenKind::FloatLiteral((n as f64) / f64::from(radix).powi(precision as i32), true)
            }
            Err(_) => TokenKind::FloatLiteral(f64::NAN, false),
        };
    }

    fn take_digits(&mut self, radix: u32) -> String {
        const DIGITS: [char; 36] = [
            '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'A', 'B', 'C', 'D', 'E', 'F', 'G',
            'H', 'I', 'J', 'K', 'L', 'M', 'N', 'O', 'P', 'Q', 'R', 'S', 'T', 'U', 'V', 'W', 'X',
            'Y', 'Z',
        ];

        let valid_digits = &DIGITS[0..(radix as usize)];

        let mut digits = String::new();
        while valid_digits.contains(&self.peek_char().to_ascii_uppercase()) {
            digits.push_str(self.take());
        }
        digits
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn empty_source() {
        let source = Source::new("test:x", "");
        let tokens = Lexer::tokenize(&source);

        assert_eq!(tokens, Arc::new(vec![Token::new(EOF, &source, 0..0)]));
    }

    #[tokio::test]
    async fn single_unknown_token() {
        let source = Source::new("test:x", "¥");
        let tokens = Lexer::tokenize(&source);

        assert_eq!(
            tokens,
            Arc::new(vec![
                Token::new(Unknown, &source, 0..1),
                Token::new(EOF, &source, 1..1),
            ])
        );
    }

    #[tokio::test]
    async fn symbol() {
        let source = Source::new("test:x", "åäöकि''");
        let tokens = Lexer::tokenize(&source);

        assert_eq!(
            tokens,
            Arc::new(vec![
                Token::new(Identifier, &source, 0..6),
                Token::new(EOF, &source, 6..6),
            ])
        );
    }

    #[tokio::test]
    async fn two_unknown_tokens() {
        let source = Source::new("test:x", "¥•");
        let tokens = Lexer::tokenize(&source);

        assert_eq!(
            tokens,
            Arc::new(vec![
                Token::new(Unknown, &source, 0..1),
                Token::new(Unknown, &source, 1..2),
                Token::new(EOF, &source, 2..2),
            ])
        );
    }

    #[tokio::test]
    async fn import_keyword() {
        let source = Source::new("test:x", "object");
        let tokens = Lexer::tokenize(&source);

        assert_eq!(
            tokens,
            Arc::new(vec![
                Token::new(ObjectKeyword, &source, 0..6),
                Token::new(EOF, &source, 6..6),
            ])
        );
    }
}
