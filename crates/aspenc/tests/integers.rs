use aspenc::{
    Expr, Lexer, Stmt, Token,
    beam::emit_program,
    ir::{Operation, lower_program},
    parse,
    types::{Type, TypedExprKind, TypedStatement, check_program},
};

#[test]
fn decimal_literals_preserve_values_spans_and_types() {
    for (source, expected) in [
        ("0", 0),
        ("-0", 0),
        ("42", 42),
        ("-42", -42),
        ("1_000_000", 1_000_000),
        ("-9_223_372_036_854_775_808", i64::MIN),
        ("9223372036854775807", i64::MAX),
    ] {
        let mut lexer = Lexer::new(source);
        let token = lexer.next().unwrap();
        assert_eq!(token.value, Token::Int(expected));
        assert_eq!(token.span.start.col, 1);
        assert_eq!(usize::from(token.span.end.col), source.len() + 1);
        assert!(lexer.next().is_none());
        assert!(lexer.diagnostics.is_empty());
        let mut diagnostics = Vec::new();
        let program = parse(Lexer::new(&format!("{source}.")), &mut diagnostics);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let Stmt::Expr(expression) = &program.statements[0].value else {
            panic!()
        };
        assert_eq!(expression.value, Expr::Int(expected));
        let typed = check_program(&program).unwrap();
        let TypedStatement::Expr(expression) = &typed[0] else {
            panic!()
        };
        assert_eq!(expression.evidence.ty, Type::Int);
        assert_eq!(expression.kind, TypedExprKind::Int(expected));
        let ir = lower_program(&typed).unwrap();
        assert_eq!(ir.entry.instructions[0].operation, Operation::Int(expected));
        assert!(
            emit_program(&ir, "integers")
                .unwrap()
                .contains(&format!("= {expected},"))
        );
    }
}

#[test]
fn malformed_or_overflowing_integers_are_diagnosed_as_whole_tokens() {
    for (source, message) in [
        ("1_", "separators"),
        ("1__0", "separators"),
        ("-1_", "separators"),
        ("9223372036854775808", "signed 64-bit"),
        ("-9223372036854775809", "signed 64-bit"),
        ("9999999999999999999999999999999999999999", "signed 64-bit"),
    ] {
        let mut lexer = Lexer::new(source);
        assert!(lexer.next().is_none());
        assert_eq!(lexer.diagnostics.len(), 1);
        let diagnostic = &lexer.diagnostics[0];
        assert!(diagnostic.message.contains(message));
        assert_eq!(diagnostic.span.start.col, 1);
        assert_eq!(usize::from(diagnostic.span.end.col), source.len() + 1);
    }
}

#[test]
fn minus_is_part_of_a_literal_only_when_adjacent_to_digits() {
    let tokens: Vec<_> = Lexer::new("-42 - 42 -> #- 42 #-42")
        .filter_map(|t| (!matches!(t.value, Token::Whitespace(_))).then_some(t.value))
        .collect();
    assert_eq!(
        tokens,
        vec![
            Token::Int(-42),
            Token::Minus,
            Token::Int(42),
            Token::Arrow,
            Token::Hash,
            Token::Minus,
            Token::Int(42),
            Token::Hash,
            Token::Int(-42)
        ]
    );
}

#[test]
fn integers_work_in_bindings_payloads_replies_and_top_constraints() {
    for source in [
        "let int x = -42. let {} y = x. y.",
        "let #value: x = #value: -42. x.",
        "{ def (int x) -> int => ^ x. } (-42).",
        "{ def go -> int => ^ -9223372036854775808. } go.",
        "{ def + (int x) -> int => ^ x. } + 1_000.",
        "{ def - (int x) -> int => ^ x. } - 42.",
    ] {
        let mut diagnostics = Vec::new();
        let parsed = parse(Lexer::new(source), &mut diagnostics);
        assert!(diagnostics.is_empty(), "{source}: {diagnostics:?}");
        let typed = check_program(&parsed).unwrap();
        emit_program(&lower_program(&typed).unwrap(), "integers").unwrap();
    }
}
