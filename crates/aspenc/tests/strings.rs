use aspenc::{
    Expr, Lexer, Stmt, Token,
    beam::emit_program,
    ir::{Operation, lower_program},
    parse,
    types::{Type, TypedExprKind, TypedStatement, check_program},
};

#[test]
fn strings_decode_and_lower_to_utf8_binaries() {
    for (source, expected) in [
        (r#""""#, ""),
        (r#""hello""#, "hello"),
        (r#""\"\\\n\r\t\0""#, "\"\\\n\r\t\0"),
        (r#""\u{41}\u{1f600}\u{10ffff}""#, "A\u{1f600}\u{10ffff}"),
        ("\"\u{e9}\u{1f600}\"", "\u{e9}\u{1f600}"),
        (r#""${not_interpolated}""#, "${not_interpolated}"),
    ] {
        let mut lexer = Lexer::new(source);
        let token = lexer.next().unwrap();
        assert_eq!(token.value, Token::String(&source[1..source.len() - 1]));
        assert_eq!(usize::from(token.span.end.col), source.chars().count() + 1);
        assert!(lexer.next().is_none());
        assert!(lexer.diagnostics.is_empty());
        let mut diagnostics = Vec::new();
        let program = parse(Lexer::new(&format!("{source}.")), &mut diagnostics);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let Stmt::Expr(expression) = &program.statements[0].value else {
            panic!()
        };
        assert_eq!(expression.value, Expr::String(expected.into()));
        let typed = check_program(&program).unwrap();
        let TypedStatement::Expr(expression) = &typed[0] else {
            panic!()
        };
        assert_eq!(expression.evidence.ty, Type::String);
        assert_eq!(expression.kind, TypedExprKind::String(expected.into()));
        let ir = lower_program(&typed).unwrap();
        assert_eq!(
            ir.entry.instructions[0].operation,
            Operation::String(expected.into())
        );
        let bytes = expected
            .as_bytes()
            .iter()
            .map(u8::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        assert!(
            emit_program(&ir, "strings")
                .unwrap()
                .contains(&format!("<<{bytes}>>"))
        );
    }
}

#[test]
fn invalid_strings_fail_lexing_and_never_produce_valid_programs() {
    for source in [
        r#""\x""#,
        r#""\u41""#,
        r#""\u{}""#,
        r#""\u{g}""#,
        r#""\u{1234567}""#,
        r#""\u{d800}""#,
        r#""\u{110000}""#,
        r#""\u{12""#,
        "\"unterminated",
        "\"trailing\\",
        "\"raw\nnewline\"",
        "\"raw\rnewline\"",
        "\"escaped\\\nnewline\"",
    ] {
        let mut lexer = Lexer::new(source);
        assert!(
            !lexer.by_ref().any(|t| matches!(t.value, Token::String(_))),
            "{source}"
        );
        assert!(!lexer.diagnostics.is_empty(), "{source}");
        assert_eq!(lexer.diagnostics[0].span.start.col, 1);
        let mut diagnostics = Vec::new();
        let program = parse(Lexer::new(&format!("{source}.")), &mut diagnostics);
        assert!(!diagnostics.is_empty());
        assert!(program.statements.is_empty());
    }
}

#[test]
fn strings_work_in_bindings_selector_payloads_replies_and_actor_top() {
    for source in [
        r#"let string x = "hello". let {} y = x. y."#,
        r#"let #value: x = #value: "hello". x."#,
        r#"{ def (string x) -> string => ^ x. } ("hello")."#,
        r#"{ def go -> string => ^ "hello". } go."#,
    ] {
        let mut diagnostics = Vec::new();
        let program = parse(Lexer::new(source), &mut diagnostics);
        assert!(diagnostics.is_empty(), "{source}: {diagnostics:?}");
        let typed = check_program(&program).unwrap();
        emit_program(&lower_program(&typed).unwrap(), "strings").unwrap();
    }
}
