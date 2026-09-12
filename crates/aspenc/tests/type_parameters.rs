use aspenc::{
    Lexer, parse,
    types::{Type, TypedExprKind, TypedStatement, check_program},
};

fn check(source: &str) -> Vec<TypedStatement> {
    let mut diagnostics = Vec::new();
    let program = parse(Lexer::new(source), &mut diagnostics);
    assert!(diagnostics.is_empty(), "{source}: {diagnostics:?}");
    check_program(&program).unwrap_or_else(|error| panic!("{source}: {error:?}"))
}

fn reject(source: &str) {
    let mut diagnostics = Vec::new();
    let program = parse(Lexer::new(source), &mut diagnostics);
    assert!(diagnostics.is_empty(), "{source}: {diagnostics:?}");
    assert!(check_program(&program).is_err(), "accepted {source}");
}

#[test]
fn explicit_parameters_infer_precise_replies_and_check_structural_bounds() {
    let typed = check(
        "let service = { def <T1, T2 <: { x -> #y }> do: T1 t1 with: T2 t2 -> T1 =>
            let #y = t2 x. ^ t1.
         }.
         service do: 42 with: { def x -> #y => ^ #y. }.",
    );
    let TypedStatement::Expr(reply) = typed.last().unwrap() else {
        panic!("expected replying send");
    };
    assert_eq!(reply.evidence.ty, Type::Int);
    reject(
        "{ def <T1, T2 <: { x -> #y }> do: T1 t1 with: T2 t2 -> T1 => ^ t1. }
         do: 42 with: {}.",
    );
    reject(
        "{ def <T1, T2 <: { x -> #y }> do: T1 t1 with: T2 t2 -> T1 => ^ t1. }
         do: 42 with: { def x -> #no => ^ #no. }.",
    );
}

#[test]
fn named_parameter_annotations_bind_exactly_without_extra_quantifiers() {
    let typed = check("{ def <T> do: T x -> T => let T y = x. ^ y. }.");
    let TypedStatement::Expr(actor) = &typed[0] else {
        panic!("expected actor");
    };
    let TypedExprKind::Actor { methods } = &actor.kind else {
        panic!("expected actor");
    };
    assert_eq!(methods[0].parameters.len(), 1);
    assert_eq!(
        methods[0].bindings[0].value.ty,
        methods[0].reply.as_ref().unwrap().ty
    );
    reject("{ def <T> do: T x -> T => ^ #different. }.");
    reject("{ def <T, U> do: T x with: U y -> T => ^ y. }.");
    reject("{ def <T> do: T x => x missing. }.");
}

#[test]
fn guarded_self_bounds_expose_capabilities_repeatedly() {
    check("{ def <T <: { next -> T }> run: T t => t next next next. }.");
    reject("{ def <T <: { next -> T }> run: T t => t missing. }.");
    reject("{ def <T <: { next -> T }> run: T t => t next. } run: {}.");
}

#[test]
fn parameter_bounds_have_whole_list_scope_and_allow_guarded_mutual_recursion() {
    check(
        "{ def <T <: { next -> U }, U <: { next -> T }> run: T t with: U u =>
             t next next next. u next next next.
         }.",
    );
    check("{ def <T <: U, U <: { next -> T }> run: T t => t next next next. }.");
    check("{ def <T <: U, U> run: T t with: U u => t. u. } run: 1 with: 2.");
}

#[test]
fn unguarded_cycles_and_duplicate_or_unknown_parameters_are_rejected() {
    for source in [
        "{ def <T <: T> run: T t => }.",
        "{ def <T <: U, U <: T> run: T t => }.",
        "{ def <T <: U, U <: V, V <: T> run: T t => }.",
        "{ def <T, T> run: T t => }.",
        "{ def <T <: Missing> run: T t => }.",
    ] {
        reject(source);
    }
}

#[test]
fn structural_polymorphic_annotations_accept_alpha_renamed_implementations() {
    let typed = check(
        "let { <T> do: T -> T } identity = { def <U> do: U x -> U => ^ x. }.
         identity do: 42.",
    );
    let TypedStatement::Expr(reply) = typed.last().unwrap() else {
        panic!("expected replying send");
    };
    assert_eq!(reply.evidence.ty, Type::Int);
    reject("let { <T> do: T -> T } identity = { def do: {} x -> {} => ^ x. }.");
    reject("let { <T, T> do: T -> T } identity = {}.");
    reject("let { <T <: T> do: T -> T } identity = {}.");
}

#[test]
fn type_parameters_are_lexical_and_calls_have_no_explicit_type_arguments() {
    reject("{ def <T> do: T x => def other: T y => }.");
    let mut diagnostics = Vec::new();
    parse(Lexer::new("service <int> do: 42."), &mut diagnostics);
    assert!(!diagnostics.is_empty());
}

#[test]
fn recursive_parameters_forward_through_generic_calls_and_lower() {
    for source in [
        "let runner = { def <T <: { next -> T }> run: T t -> T => ^ t next next. }. { def <U <: { next -> U }> use: U u -> U => ^ runner run: u. }.",
        "let { <T <: { next -> T }> run: T -> T } runner = { def <U <: { next -> U }> run: U u -> U => ^ u next. }.",
        "{ def <T <: #box: T> run: T t => t. }.",
        "{ def <T <: { <U <: T> get: U -> T }> run: T t => t. }.",
    ] {
        let typed = check(source);
        let ir = aspenc::ir::lower_program(&typed).unwrap();
        aspenc::beam::emit_program(&ir, "recursive_types").unwrap();
    }
}

#[test]
fn whole_list_bounds_are_validated_after_forward_references_resolve() {
    check("{ def <T <: { (U). (V) }, U <: int, V <: string> run: T t => }.");
    reject("{ def <T <: { (U). (V) }, U <: int, V <: int> run: T t => }.");
}

#[test]
fn unobserved_parameters_default_independently_of_declaration_order() {
    check("{ def <T <: U, U> run: T t => } run: 1.");
    check("{ def <U, T <: U> run: T t => } run: 1.");
    check("{ def <T <: U, U <: V, V> run: T t => } run: 1.");
    check("{ def <T <: { next -> T }> start => } start.");
}
