# Aspen

The agreed BEAM runtime contract and compiler milestones are documented in
[`docs/beam-runtime.md`](docs/beam-runtime.md).

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
cargo run -p aspenc -- lower example.aspen
printf 'let service = { def (x) => x. }. service (#home).' | cargo run -q -p aspenc -- check -
```

- `lex` prints located tokens, including whitespace. It reports lexical errors
  while continuing to print the tokens it can recognize.
- `parse` prints the located AST only if parsing succeeds.
- `check` prints `ok` on success. Programs are statement sequences, not values.
  `--typed-ast` instead prints the typed AST, including type evidence and bindings.
  Files with lexical or parse errors are not passed to the type checker.
- `lower` checks a program and prints the backend-independent executable IR.
  It does not execute the program or generate BEAM code. The dump exposes
  sequencing, message sends, captures, projections, and static adaptation plans.

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
a value reply and is a different contract. Methods declare replies explicitly:

```text
let service = {
  def ready -> #done =>
    ^ #done.
    ^ #done.
}.
service ready.
```

Inside an annotated method, `^` names its reply-to actor, with type `{ (type) }`
for the declared reply type. Direct `^` sends parse their message in ordinary
expression mode: `^ x` replies with variable `x`, `^ #done` replies with the
selector, and `^ service get` replies with the value of `service get`. Bare `^`
still names the actor; aliases use ordinary actor-send syntax.
Sending to it is an ordinary send: it does not exit
the method. Zero or multiple replies are permitted; the annotation constrains
reply messages, not their number or delivery.

An unannotated method has no reply and cannot use `^`. Each nested method gets
its own reply-to actor only if annotated; to capture an enclosing reply-to actor,
first bind an explicit alias with `let reply_to = ^.`.

A no-reply send can stand alone as a statement, but cannot initialize a binding
or supply an expression value. This includes sends to `^` itself.
