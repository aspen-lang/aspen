use aspenc::{Lexer, parse, types::check_program};

fn result(source: &str) -> Result<(), String> {
    let mut diagnostics = Vec::new();
    let program = parse(Lexer::new(source), &mut diagnostics);
    assert!(diagnostics.is_empty(), "{source}: {diagnostics:?}");
    check_program(&program)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[test]
fn transparent_forward_generic_and_separate_namespaces() {
    for source in [
        "let Name Name = 42. type Name int.",
        "type A B. type B string. let A x = \"ok\".",
        "type Box<T> { get -> T. }. let Box<string> x = { def get -> string => ^ \"ok\". }. x get.",
        "type Box<T <: int> T. let Box<int> x = 42.",
        "type List { head -> int. tail -> List. }. { def use: List x -> int => ^ x tail head. }.",
        "type A { next -> B. }. type B { next -> A. }.",
        "type A<T> { next -> A<T>. }. type B A<string>.",
        "type A { next -> A. }. type B { next -> B. }. { def use: A x -> B => ^ x. }.",
        "type Box<T> #box: T. { def <T> use: Box<T> x -> Box<T> => ^ x. } use: #box: 1.",
        "type A<T> { next -> A<T>. }. { def <T> use: A<T> x -> A<T> => ^ x next. }.",
    ] {
        assert!(result(source).is_ok(), "{source}: {:?}", result(source));
    }
}

#[test]
fn aliases_reject_invalid_definitions_and_applications() {
    for source in [
        "type A A.",
        "type A B. type B A.",
        "type A Missing.",
        "type A int. type A string.",
        "type A<T> T. let A x = 1.",
        "type A int. let A<int> x = 1.",
        "type A<T <: int> T. let A<string> x = \"no\".",
        "type A<T> { next -> A<#x: T>. }.",
        "type A<T> A<T>.",
    ] {
        assert!(result(source).is_err(), "accepted {source}");
    }
}
