//! End-to-end foundation fixtures deliberately hide concrete actors behind bounds.
use aspenc::{Lexer, ir::lower_program, parse, types::check_program};

#[test]
fn existing_structural_fragment_lowers_without_runtime_adapters() {
    for source in [
        // Interface start is not implementation slot zero.
        "let use = { def use: ({ start -> #ok } a) => a start. }.\
         use use: { def stop -> #no => ^ #no. def start -> #ok => ^ #ok. }.",
        // One generic receiver supplies multiple required signatures.
        "let use = { def use: ({ start. stop } a) => a start. a stop. }.\
         use use: { def (x) => }.",
        // Structural obligations under selectors and inside actor inputs.
        "let use = { def use: ({ accept: ({ ready }) } a) =>\
           a accept: { def ready => }. }.\
         use use: { def accept: ({} actor) => }.",
        // Distinct selector tags and nested payloads share an outer selector.
        "let a = { def put: (#nested: x) => def put: #left => def put: #right => }.\
         a put: (#nested: {}). a put: #left. a put: #right.",
        // A first-class reply handle crosses an actor boundary unchanged.
        "let delegate = { def report: ({ (#ok) } target) => target (#ok). }.\
         let service = { def start -> #ok => delegate report: ^. }.\
         service start.",
        // A lexical reply alias survives into an unannotated nested method.
        "let service = { def start -> #ok => let target = ^.\
           let nested = { def report => target (#ok). }. nested report. }.\
         service start.",
        // Reply covariance permits actor results with additional capabilities.
        "let use = { def use: ({ get -> { ready } } a) =>\
           let result = a get. result ready. }.\
         use use: { def get -> { ready. stop } =>\
           ^ { def ready => def stop => }. }.",
        // Current cardinality rules must not be narrowed by executable lowering.
        "let a = { def empty -> #ok => def twice -> #ok => ^ #ok. ^ #ok. }.\
         a twice. a empty.",
    ] {
        let mut diagnostics = Vec::new();
        let parsed = parse(Lexer::new(source), &mut diagnostics);
        assert!(diagnostics.is_empty(), "{source}: {diagnostics:?}");
        let typed = check_program(&parsed).unwrap_or_else(|error| panic!("{source}: {error}"));
        lower_program(&typed).unwrap_or_else(|error| panic!("{source}: {error}"));
    }
}
