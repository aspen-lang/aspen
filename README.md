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
printf '{ def (x) => x } (#home)' | cargo run -q -p aspenc -- check -
```

- `lex` prints located tokens, including whitespace. It reports lexical errors
  while continuing to print the tokens it can recognize.
- `parse` prints the located AST only if parsing succeeds.
- `check` prints the inferred type on success (`#home` in the stdin example).
  `--typed-ast` instead prints the typed AST, including type evidence and bindings.
  Files with lexical or parse errors are not passed to the type checker.

Each command accepts one UTF-8 file; `-` reads standard input. Debug output goes to
stdout, and diagnostics go to stderr as `file:line:column: error: message`, with
related source locations where available. Lines and Unicode scalar columns are
one-based. The type checker currently stops at the first type error.

Exit status is 0 for success, 1 for source or I/O errors, and 2 for invalid CLI
arguments. AST and token dumps use Rust debug formatting and are intended for
inspection, not as a stable machine-readable format.
