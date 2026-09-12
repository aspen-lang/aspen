//! File-level declarations and imports. Module identities come from file paths.
use crate::{parser::Parser, *};

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ModuleSyntax {
    pub imports: Vec<Loc<ImportSyntax>>,
    pub globals: Vec<Loc<GlobalSyntax>>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ImportSyntax {
    pub path: Loc<String>,
    pub binding: ImportBinding,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ImportBinding {
    Module(Loc<String>),
    Names(Vec<ImportedName>),
}

#[derive(Debug, PartialEq, Eq)]
pub struct ImportedName {
    pub name: Loc<String>,
    pub alias: Loc<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct GlobalSyntax {
    pub exported: bool,
    pub binding: Let,
}

fn name(parser: &mut Parser<'_, '_>) -> Option<Loc<String>> {
    let Some(Token::Identifier(value)) = parser.peek() else {
        return parser.error("expected name");
    };
    let token = parser.bump();
    Some(Loc {
        value: value.into(),
        span: token.span,
    })
}

fn import(parser: &mut Parser<'_, '_>) -> Option<Loc<ImportSyntax>> {
    let start = parser.bump().span.start;
    let mut last = name(parser)?;
    let mut path = last.clone();
    while parser.peek() == Some(Token::Slash) {
        let slash = parser.bump();
        if slash.span.start != path.span.end || slash.span.end != parser.span().start {
            return parser.error("import paths require adjacent '/' and names");
        }
        last = name(parser)?;
        path.value.push('/');
        path.value.push_str(&last.value);
        path.span.end = last.span.end;
    }
    let binding = if parser.eat(Token::Identifier("as")) {
        ImportBinding::Module(name(parser)?)
    } else if parser.eat(Token::OpenParen) {
        let mut names = Vec::new();
        loop {
            let imported = name(parser)?;
            let alias = if parser.eat(Token::Identifier("as")) {
                name(parser)?
            } else {
                imported.clone()
            };
            names.push(ImportedName {
                name: imported,
                alias,
            });
            if !parser.eat(Token::Comma) {
                break;
            }
        }
        parser.expect(Token::CloseParen, "expected ')' after imported names")?;
        ImportBinding::Names(names)
    } else {
        ImportBinding::Module(last)
    };
    parser.expect(Token::Dot, "expected '.' after import")?;
    Some(parser.located(start, ImportSyntax { path, binding }))
}

fn simple_binding(pattern: &Loc<Pattern>) -> bool {
    match &pattern.value {
        Pattern::Variable(_) => true,
        Pattern::Annotated { pattern, .. } => matches!(pattern.value, Pattern::Variable(_)),
        _ => false,
    }
}

fn static_expression(expr: &Loc<Expr>, diagnostics: &mut Vec<Diagnostic>) {
    match &expr.value {
        Expr::Int(_) | Expr::Float(_) | Expr::String(_) | Expr::Variable(_) | Expr::Actor(_) => {},
        Expr::Selector(selector) => {
            for value in selector.values() {
                static_expression(value, diagnostics);
            }
        },
        Expr::Send { .. } | Expr::ReplyTo => diagnostics.push(Diagnostic {
            span: expr.span,
            message: "global initializer must be static (message sends and reply targets are only allowed inside actors)".into(),
        }),
    }
}

/// Parse a module, preserving declaration spans including their final periods.
/// Errors are appended to `diagnostics`; callers must reject erroneous modules.
pub fn parse_module(lexer: Lexer<'_>, diagnostics: &mut Vec<Diagnostic>) -> ModuleSyntax {
    let (mut parser, invalid) = Parser::new(lexer, diagnostics);
    let mut module = ModuleSyntax::default();
    while parser.peek().is_some() {
        if parser.peek() == Some(Token::Identifier("import")) {
            let Some(import) = import(&mut parser) else {
                break;
            };
            module.imports.push(import);
            continue;
        }
        let start = parser.span().start;
        let exported = parser.eat(Token::Identifier("export"));
        if parser.peek() != Some(Token::Let) {
            parser.error::<()>(if exported {
                "expected 'let' after 'export'"
            } else {
                "expected import, let, or export let declaration at module top level"
            });
            break;
        }
        let Some(statement) = parser.statement() else {
            break;
        };
        let Stmt::Let(binding) = statement.value else {
            unreachable!()
        };
        if !simple_binding(&binding.pattern) {
            parser.diagnostics.push(Diagnostic {
                span: binding.pattern.span,
                message: "global declaration requires a simple name, optionally type-annotated"
                    .into(),
            });
        }
        static_expression(&binding.value, parser.diagnostics);
        module.globals.push(Loc {
            value: GlobalSyntax { exported, binding },
            span: Span {
                start,
                end: statement.span.end,
            },
        });
    }
    if invalid {
        ModuleSyntax::default()
    } else {
        module
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(source: &str) -> (ModuleSyntax, Vec<Diagnostic>) {
        let mut diagnostics = Vec::new();
        let module = parse_module(Lexer::new(source), &mut diagnostics);
        (module, diagnostics)
    }

    #[test]
    fn imports_and_globals_preserve_bindings_and_spans() {
        let (module, diagnostics) = parsed(
            "import pkg/sub/module.\nimport pkg/sub as alias.\nimport pkg/other (global, other_global as renamed).\nlet int private = 1.\nexport let public = alias/global.",
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(module.imports.len(), 3);
        assert_eq!(module.imports[0].path.value, "pkg/sub/module");
        assert!(
            matches!(&module.imports[0].binding, ImportBinding::Module(alias) if alias.value == "module")
        );
        assert!(
            matches!(&module.imports[1].binding, ImportBinding::Module(alias) if alias.value == "alias")
        );
        let ImportBinding::Names(names) = &module.imports[2].binding else {
            panic!()
        };
        assert_eq!(names[0].alias.value, "global");
        assert_eq!(names[1].name.value, "other_global");
        assert_eq!(names[1].alias.value, "renamed");
        assert!(!module.globals[0].exported);
        assert!(module.globals[1].exported);
        assert_eq!(module.globals[1].span.start, Pos { line: 5, col: 1 });
        assert_eq!(
            module.globals[1].binding.value.value,
            Expr::Variable("alias/global".into())
        );
    }

    #[test]
    fn static_values_allow_actor_bodies_and_recursive_selectors() {
        for source in [
            "",
            "let x = y. let y = 1.",
            "let x = #pair: 1 with: #ok.",
            "export let main = { def _ => io/console print: 1. }.",
            "let x = #actor: { def _ => ^ 1. }.",
        ] {
            let (_, diagnostics) = parsed(source);
            assert!(diagnostics.is_empty(), "{source}: {diagnostics:?}");
        }
        for source in [
            "let x = 1 + 2.",
            "let x = #value: (f run).",
            "let x = ^.",
            "let _ = 1.",
            "let #foo = 1.",
            "{}. ",
            "x.",
            "export import p/m.",
        ] {
            let (_, diagnostics) = parsed(source);
            assert!(!diagnostics.is_empty(), "{source}");
        }
    }

    #[test]
    fn malformed_imports_are_rejected() {
        for source in [
            "import.",
            "import p/.",
            "import p /m.",
            "import p/ m.",
            "import p as.",
            "import p ().",
            "import p (x,).",
            "import p (x y).",
            "import p (x as).",
            "import p",
        ] {
            let (_, diagnostics) = parsed(source);
            assert!(!diagnostics.is_empty(), "{source}");
        }
    }

    #[test]
    fn slash_adjacency_disambiguates_names_from_division() {
        for (source, valid) in [
            ("a/b.", true),
            ("a / b.", true),
            ("a/ b.", false),
            ("a /b.", false),
        ] {
            let mut diagnostics = Vec::new();
            let program = parse(Lexer::new(source), &mut diagnostics);
            assert_eq!(diagnostics.is_empty(), valid, "{source}: {diagnostics:?}");
            if source == "a/b." {
                assert!(
                    matches!(&program.statements[0].value, Stmt::Expr(expr) if expr.value == Expr::Variable("a/b".into()))
                );
            }
            if source == "a / b." {
                assert!(
                    matches!(&program.statements[0].value, Stmt::Expr(expr) if matches!(expr.value, Expr::Send { .. }))
                );
            }
        }
    }
}
