use ansi_colors::ColouredStr;
use aspen::syntax::{Lexer, Token, TokenKind};
use aspen::Diagnostics;
use std::collections::HashMap;
use std::sync::Arc;

pub fn report(diagnostics: Diagnostics) {
    if diagnostics.is_empty() {
        return;
    }

    let mut heading = ColouredStr::new(" DIAGNOSIS ");
    heading.back_light_red();
    heading.black();
    heading.bold();

    print!("{}\n\n", heading);

    let mut groups: Vec<_> = diagnostics.group_by_source().into_iter().collect();

    groups.sort_by(|(a, _), (b, _)| a.uri().cmp(&b.uri()));

    for (source, diagnostics) in groups {
        let uri = format!("{:?}", source.uri());
        let mut uri = ColouredStr::new(uri.as_str());
        uri.dark_gray();
        println!("{}", uri);

        let mut diagnostics: Vec<_> = diagnostics.into_iter().collect();

        let tokens = Lexer::tokenize(&source);
        let pairs: Vec<(&Arc<Token>, Diagnostics)> = tokens
            .iter()
            .map(|token| {
                (
                    token,
                    diagnostics
                        .drain_filter(|d| d.range() == &token.range)
                        .collect(),
                )
            })
            .collect();

        let mut lines: HashMap<usize, Vec<(&Arc<Token>, Diagnostics)>> = HashMap::new();

        for (token, diagnostics) in pairs {
            if !lines.contains_key(&token.range.start.line) {
                lines.insert(token.range.start.line, vec![]);
            }
            lines
                .get_mut(&token.range.start.line)
                .unwrap()
                .push((token, diagnostics))
        }

        let mut lines: Vec<_> = lines.into_iter().collect();

        lines.sort_by(|(a, _), (b, _)| a.cmp(b));

        let gutter_width = lines.len().to_string().len();

        for (line_number, tokens) in lines {
            print!(
                "{:gutter_width$} | ",
                line_number,
                gutter_width = gutter_width
            );
            for (token, d) in tokens.iter() {
                let mut lexeme = token.lexeme();
                if lexeme == "\n" {
                    lexeme = " ";
                }
                let mut lexeme = ColouredStr::new(lexeme);

                if !d.is_empty() {
                    lexeme.red();
                    lexeme.underline();
                } else {
                    use TokenKind::*;
                    match token.kind {
                        ObjectKeyword | ClassKeyword => {
                            lexeme.blue();
                        }
                        _ => {}
                    }
                }

                print!("{}", lexeme);
            }
            print!("\n");
            for (token, diagnostics) in tokens {
                for diagnostic in diagnostics {
                    let mut message = diagnostic.message();
                    message.insert(0, '^');
                    message.insert(1, ' ');
                    let mut message = ColouredStr::new(message.as_str());
                    message.red();
                    print!(
                        "  | {}{}\n",
                        " ".repeat(token.range.start.character - 1),
                        message
                    )
                }
            }
        }
    }
}
