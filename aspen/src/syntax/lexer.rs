use crate::source::Source;
use crate::syntax::{Token, TokenKind, TokenKind::*};
use peekmore::{PeekMore, PeekMoreIterator};
use std::sync::Arc;
use crate::Graphemes;

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

    fn peek_char(&mut self) -> char {
        self.peek().chars().next().unwrap_or(0 as char)
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
        match self.take_symbol() {
            "object" => ObjectKeyword,
            "class" => ClassKeyword,
            _ => Identifier,
        }
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
