use aspenc::{
    Lexer,
    beam::emit_program,
    ir::lower_program,
    parse,
    types::{Type, TypeError, TypedStatement, check_program},
};

fn check(source: &str) -> Result<Vec<TypedStatement>, TypeError> {
    let mut diagnostics = Vec::new();
    let program = parse(Lexer::new(source), &mut diagnostics);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    check_program(&program)
}

fn with_syscall(source: &str) -> String {
    format!("let syscall = {{ def write: int fd data: bytes data -> int => ^ 0. }}. {source}")
}

#[test]
fn syscall_is_not_ambient_but_remains_an_ordinary_binding() {
    assert!(matches!(
        check("syscall."),
        Err(TypeError::UnboundVariable { .. })
    ));
    assert!(matches!(
        check("{ def run => syscall. }."),
        Err(TypeError::UnboundVariable { .. })
    ));
    for source in [
        r#"syscall write: 1 data: "hello\n"."#,
        r#"let os = syscall. os write: 2 data: "hello"."#,
        r#"{ def run -> int => ^ syscall write: 1 data: "hello". } run."#,
        r#"let syscall = syscall. { def run -> int => ^ syscall write: 1 data: "hello". } run."#,
        r#"let { write: int data: bytes -> int } os = syscall. os write: 1 data: "hi"."#,
    ] {
        let typed = check(&with_syscall(source)).unwrap();
        let emitted = emit_program(&lower_program(&typed).unwrap(), "syscalls").unwrap();
        assert!(!emitted.contains("aspen_runtime:syscall(_Session)"));
    }
    let shadow = check("let syscall = 42. syscall.").unwrap();
    let TypedStatement::Expr(value) = &shadow[1] else {
        panic!()
    };
    assert_eq!(value.evidence.ty, Type::Int);
    for source in [
        r#"syscall write: "stdout" data: "hi"."#,
        "syscall write: 1 data: 42.",
    ] {
        assert!(matches!(
            check(&with_syscall(source)),
            Err(TypeError::NoReceiver { .. })
        ));
    }
}

#[test]
fn strings_are_bytes_but_bytes_do_not_promise_utf8() {
    check(&with_syscall(
        r#"let bytes data = "hi". syscall write: 1 data: data."#,
    ))
    .unwrap();
    check(&with_syscall(
        r#"{ def (bytes data) -> int => ^ syscall write: 1 data: data. } ("hi")."#,
    ))
    .unwrap();
    assert!(matches!(
        check("{ def (bytes data) => let string text = data. }."),
        Err(TypeError::Mismatch(_))
    ));
    assert!(matches!(
        check("{ def (bytes data) => def (string text) => }."),
        Err(TypeError::OverlappingReceivers { .. })
    ));
}
