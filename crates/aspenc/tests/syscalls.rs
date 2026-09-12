use aspenc::{
    Lexer,
    beam::emit_program,
    ir::{Operation, lower_program},
    parse,
    types::{Type, TypeError, TypedStatement, check_program},
};

fn check(source: &str) -> Result<Vec<TypedStatement>, TypeError> {
    let mut diagnostics = Vec::new();
    let program = parse(Lexer::new(source), &mut diagnostics);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    check_program(&program)
}

#[test]
fn syscall_is_global_first_class_and_shadowable() {
    for source in [
        r#"syscall write: 1 data: "hello\n"."#,
        r#"let os = syscall. os write: 2 data: "hello"."#,
        r#"{ def run -> int => ^ syscall write: 1 data: "hello". } run."#,
        r#"let syscall = syscall. { def run -> int => ^ syscall write: 1 data: "hello". } run."#,
        r#"let { write: int data: bytes -> int } os = syscall. os write: 1 data: "hi"."#,
    ] {
        let typed = check(source).unwrap();
        emit_program(&lower_program(&typed).unwrap(), "syscalls").unwrap();
    }
    let typed = check("syscall.").unwrap();
    let ir = lower_program(&typed).unwrap();
    assert_eq!(ir.entry.instructions[0].operation, Operation::Syscall);
    assert!(
        emit_program(&ir, "syscalls")
            .unwrap()
            .contains("aspen_runtime:syscall(_Session)")
    );
    let shadow = check("let syscall = 42. syscall.").unwrap();
    let TypedStatement::Expr(value) = &shadow[1] else {
        panic!()
    };
    assert_eq!(value.evidence.ty, Type::Int);
    assert!(matches!(
        check(r#"syscall write: "stdout" data: "hi"."#),
        Err(TypeError::NoReceiver { .. })
    ));
    assert!(matches!(
        check("syscall write: 1 data: 42."),
        Err(TypeError::NoReceiver { .. })
    ));
}

#[test]
fn strings_are_bytes_but_bytes_do_not_promise_utf8() {
    check(r#"let bytes data = "hi". syscall write: 1 data: data."#).unwrap();
    check(r#"{ def (bytes data) -> int => ^ syscall write: 1 data: data. } ("hi")."#).unwrap();
    assert!(matches!(
        check("{ def (bytes data) => let string text = data. }."),
        Err(TypeError::Mismatch(_))
    ));
    assert!(matches!(
        check("{ def (bytes data) => def (string text) => }."),
        Err(TypeError::OverlappingReceivers { .. })
    ));
}
