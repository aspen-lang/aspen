use aspenc::{
    Expr, FloatValue, Lexer, Stmt, Token,
    beam::emit_program,
    ir::{Operation, lower_program},
    parse,
    types::{Type, TypedExprKind, TypedStatement, check_program},
};

#[test]
fn floats_preserve_binary64_values_through_lowering() {
    for (source, expected) in [
        ("0.0", 0.0_f64),
        ("-0.0", -0.0),
        ("01.50", 1.5),
        ("-1_000.25", -1000.25),
        ("1e3", 1000.0),
        ("1E+3", 1000.0),
        ("1_2.3_4e-0_2", 0.1234),
        ("1.7976931348623157e308", f64::MAX),
        ("2.2250738585072014e-308", f64::MIN_POSITIVE),
        ("5e-324", f64::from_bits(1)),
        ("-1e-999", -0.0),
        ("9223372036854775808.0", 9223372036854775808.0),
    ] {
        let value = FloatValue::new(expected).unwrap();
        let mut lexer = Lexer::new(source);
        let token = lexer.next().unwrap();
        assert_eq!(token.value, Token::Float(value), "{source}");
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
        assert_eq!(expression.value, Expr::Float(value));
        let typed = check_program(&program).unwrap();
        let TypedStatement::Expr(expression) = &typed[0] else {
            panic!()
        };
        assert_eq!(expression.evidence.ty, Type::Float);
        assert_eq!(expression.kind, TypedExprKind::Float(value));
        let ir = lower_program(&typed).unwrap();
        assert_eq!(ir.entry.instructions[0].operation, Operation::Float(value));
        assert!(
            emit_program(&ir, "floats")
                .unwrap()
                .contains(&format!("= {value},"))
        );
        assert_eq!(
            value.to_string().parse::<f64>().unwrap().to_bits(),
            expected.to_bits()
        );
    }
    assert!(FloatValue::new(f64::NAN).is_none());
    assert!(FloatValue::new(f64::INFINITY).is_none());
    assert!(FloatValue::new(f64::NEG_INFINITY).is_none());
    assert_ne!(FloatValue::new(0.0), FloatValue::new(-0.0));
}

#[test]
fn malformed_floats_are_diagnosed_as_whole_tokens() {
    for source in [
        "1.0_", "1_.0", "1.0__2", "1e_2", "1e2_", "1e", "1E+", "-1.5e-", "1e309", "-1e309",
    ] {
        let mut lexer = Lexer::new(source);
        assert!(lexer.next().is_none(), "{source}");
        assert_eq!(lexer.diagnostics.len(), 1, "{source}");
        let diagnostic = &lexer.diagnostics[0];
        assert!(diagnostic.message.contains("float"), "{diagnostic:?}");
        assert_eq!(diagnostic.span.start.col, 1);
        assert_eq!(usize::from(diagnostic.span.end.col), source.len() + 1);
    }
}

#[test]
fn decimal_points_and_minus_preserve_statement_and_operator_syntax() {
    let tokens: Vec<_> = Lexer::new("1. 1.5. .5 -1.5 - 1.5 -> #- 1.5")
        .filter_map(|t| (!matches!(t.value, Token::Whitespace(_))).then_some(t.value))
        .collect();
    let float = |value| Token::Float(FloatValue::new(value).unwrap());
    assert_eq!(
        tokens,
        vec![
            Token::Int(1),
            Token::Dot,
            float(1.5),
            Token::Dot,
            Token::Dot,
            Token::Int(5),
            float(-1.5),
            Token::Minus,
            float(1.5),
            Token::Arrow,
            Token::Hash,
            Token::Minus,
            float(1.5)
        ]
    );
}

#[test]
fn floats_work_in_payloads_replies_and_actor_top_without_integer_coercion() {
    for source in [
        "let float x = -42.5. let {} y = x. y.",
        "let #value: x = #value: 1e3. x.",
        "{ def (float x) -> float => ^ x. } (-42.5).",
        "{ def go -> float => ^ 1e3. } go.",
        "{ def + (float x) -> float => ^ x. } + 1_000.5.",
    ] {
        let mut diagnostics = Vec::new();
        let parsed = parse(Lexer::new(source), &mut diagnostics);
        assert!(diagnostics.is_empty(), "{source}: {diagnostics:?}");
        let typed = check_program(&parsed).unwrap();
        emit_program(&lower_program(&typed).unwrap(), "floats").unwrap();
    }
    for source in ["let int x = 1.0.", "let float x = 1."] {
        let mut diagnostics = Vec::new();
        let parsed = parse(Lexer::new(source), &mut diagnostics);
        assert!(diagnostics.is_empty());
        assert!(check_program(&parsed).is_err());
    }
}
