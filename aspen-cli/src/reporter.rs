use ansi_colors::ColouredStr;
use aspen::syntax::{Lexer, Token, TokenKind};
use aspen::{Diagnostic, Diagnostics};
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

        let diagnostics: Vec<_> = diagnostics.into_iter().collect();

        let tokens = Lexer::tokenize(&source);
        let pairs: Vec<(&Arc<Token>, bool, Vec<&Arc<dyn Diagnostic>>)> = tokens
            .iter()
            .map(|token| {
                (
                    token,
                    diagnostics.iter().any(|d| d.range().contains(&token.range)),
                    diagnostics
                        .iter()
                        .filter(|d| d.range().start == token.range.start)
                        .collect(),
                )
            })
            .collect();

        let mut lines: HashMap<usize, Vec<(&Arc<Token>, bool, Vec<&Arc<dyn Diagnostic>>)>> =
            HashMap::new();

        for (token, has_error, diagnostics) in pairs {
            if !lines.contains_key(&token.range.start.line) {
                lines.insert(token.range.start.line, vec![]);
            }
            lines
                .get_mut(&token.range.start.line)
                .unwrap()
                .push((token, has_error, diagnostics))
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
            for (token, has_error, _) in tokens.iter() {
                let mut lexeme = token.lexeme();
                if lexeme == "\n" {
                    lexeme = " ";
                }
                let mut lexeme = ColouredStr::new(lexeme);

                if *has_error {
                    lexeme.red();
                    lexeme.underline();
                } else {
                    use TokenKind::*;
                    match token.kind {
                        ObjectKeyword | ClassKeyword | InstanceKeyword | OfKeyword => {
                            lexeme.blue();
                        }
                        _ => {}
                    }
                }

                print!("{}", lexeme);
            }
            print!("\n");
            for (token, _, diagnostics) in tokens {
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
