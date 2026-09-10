# Aspen

Aspen is an experimental language with structural actors, selectors, message
sends, and bounded method polymorphism. The compiler currently implements lexing,
parsing, and static type checking; it does not execute programs or generate code.
The current language fragment is specified in `spec/type-system.typ`.

## Development

Use the Nix development shell for the compiler's Rust toolchain:

```sh
nix develop
cargo test --workspace --locked
```

## Compiler Debugging CLI

```sh
cargo run -p aspenc -- --help
cargo run -p aspenc -- lex example.aspen
cargo run -p aspenc -- parse example.aspen
cargo run -p aspenc -- check example.aspen
cargo run -p aspenc -- check --typed-ast example.aspen
printf 'let service = { def (x) => x. }. service (#home).' | cargo run -q -p aspenc -- check -
```

- `lex` prints located tokens, including whitespace. It reports lexical errors
  while continuing to print the tokens it can recognize.
- `parse` prints the located AST only if parsing succeeds.
- `check` prints `ok` on success. Programs are statement sequences, not values.
  `--typed-ast` instead prints the typed AST, including type evidence and bindings.
  Files with lexical or parse errors are not passed to the type checker.

Each command accepts one UTF-8 file; `-` reads standard input. Debug output goes to
stdout, and diagnostics go to stderr as `file:line:column: error: message`, with
related source locations where available. Lines and Unicode scalar columns are
one-based. The type checker currently stops at the first type error.

Exit status is 0 for success, 1 for source or I/O errors, and 2 for invalid CLI
arguments. AST and token dumps use Rust debug formatting and are intended for
inspection, not as a stable machine-readable format.

## Statements And Replies

Programs and method bodies contain zero or more period-terminated statements:

```text
let service = {
  def put: value =>
    let saved = value.
    saved.
  def ready =>
}.
service put: #home.
```

`let` introduces bindings for subsequent statements in its sequence. Expression
statements discard their values; even a method's final expression does not
implicitly send a reply. Method-local bindings do not escape their method.

A signature such as `{put: any. ready}` has no replies. `{ready -> {}}` promises
a value reply and is a different contract. Actor expressions currently infer
only no-reply signatures, pending an explicit reply statement. A no-reply send
can stand alone as a statement, but cannot initialize a binding or supply an
expression value.
