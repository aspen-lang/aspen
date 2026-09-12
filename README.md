# Aspen

The agreed BEAM runtime contract and compiler milestones are documented in
[`docs/beam-runtime.md`](docs/beam-runtime.md).

Aspen is an experimental language with structural actors, selectors, message
sends, and bounded method polymorphism. The compiler currently implements lexing,
parsing, static type checking, Erlang source generation, and execution on BEAM.
The current language fragment is specified in `spec/type-system.typ`.

Every value is an actor in the type system, including primitives. The unit actor
`{}` is the top type, and `never` is bottom; there is no separate `any` type.
The primitive types `bytes`, `int`, `float`, and `selector` are subtypes of `{}`.
Strings refine immutable byte sequences: `string <: bytes <: {}`. A `bytes`
argument accepts a string directly, without conversion; arbitrary bytes need not
be valid UTF-8. There is no separate byte-literal syntax yet.
Selectors divide into `atom`, `optagged`, and `keywordtagged`, with concrete
selector types below their respective families (for example,
`#ready <: atom <: selector <: {}`). These type names are available in
annotations. Decimal signed 64-bit integer literals are supported, including
negative values and underscore separators: `42`, `-42`, `1_000`. Underscores must
occur singly between digits; the minus sign must touch the digits. Strings use
double quotes, contain UTF-8 text, and support `\"`, `\\`, `\n`, `\r`, `\t`, `\0`,
and `\u{...}` escapes. Raw newlines and interpolation are not supported.
Floats use finite IEEE 754 binary64 values: `1.5`, `1e3`, `-1_000.25e-2`.
They follow the same sign and separator rules as integers; decimal points require
digits on both sides, leaving `1.` as an integer followed by a statement terminator.
Overflow is rejected; rounding and underflow to signed zero are allowed.

## Method Type Parameters

Methods can name their type parameters explicitly. Bounds default to `{}`, and
calls infer type arguments from the message:

```aspen
export let service = {
  def <T1, T2 <: { x -> #y }> do: T1 t1 with: T2 t2 -> T1 =>
    let #y = t2 x.
    ^ t1.
}.
```

`T1 t1` binds `t1` at exactly type `T1`, without introducing another parameter.
The reply retains the caller's inferred type rather than widening to `{}`.
Structural actor annotations use the same syntax: `{ <T> do: T -> T }`.
There is no explicit type-argument syntax on calls.

All parameters are in scope throughout their list's bounds. Guarded self and
mutual bounds are supported, for example
`def <T <: { next -> T }> run: T t => t next next next.` inside an actor.
Bare cycles such as `<T <: T>` or `<T <: U, U <: T>` are rejected. These are
F-bounded subtype constraints, not equirecursive equalities: the bound exposes
methods on `T` but does not make `T` equal to its bound. Unannotated receiver
variables continue to introduce implicit bounded parameters.

## Type Aliases

Aliases are transparent, private by default, and live in a separate namespace:

```aspen
type Text string.
export type Box<T> { get -> T. }.
type Chain { value -> int. next -> Chain. }.
```

Use `Box<string>` to supply explicit type arguments. Bounds use `<T <: Other>`;
omitted bounds are `{}`. Forward references and guarded mutual recursion are
supported, with implicit unfolding on either side of type comparisons. Bare
recursive cycles and recursive applications that change their type arguments
are rejected. Aliases have no runtime representation.

Import types explicitly with `import app/model (value, type Box as LocalBox).`,
or access exported types through a module import, such as `model/Box<string>`.
See [modules](docs/modules.md) for namespace and visibility details.

## Low-Level Output

The global actor `syscall` exposes real OS syscalls. For example:

```aspen
export let main = {
  def start => syscall write: 1 data: "Hello, world!\n".
}.
```

Its interface is `{ write: int data: bytes -> int }`. `write` attempts one POSIX
write and replies with the byte count, or negative native `errno` on failure.
It does not add a newline, retry interrupted or partial writes, or interpret the
bytes as text. Descriptors refer to the BEAM process: `1` is normally stdout and
`2` stderr. This is unrestricted low-level descriptor access, not an I/O sandbox.
A discarded reply still waits for the write attempt to finish before execution
continues.

`syscall` is a first-class actor shared within each runtime session. It is available
inside methods as well as at top level; lexical bindings can shadow it, and it can
be passed or aliased like any other actor.

## Modules

Each source file defines a module, with private `let` and public `export let`
globals. Initializers are declarative; effects run only inside actor methods.
Modules import exported globals using package paths. A YAML package manifest
selects the source root and a global actor plus an initial no-reply atomic
message. `index.aspen` names its containing directory's module.

See [`docs/modules.md`](docs/modules.md) for the manifest, import syntax,
qualified references, recursive module checking, and global identity rules.
Script-style top-level expression statements are no longer accepted.

## Development

Use the Nix development shell for the compiler's Rust and Erlang/OTP 28 toolchains.
The native syscall bridge also requires a C compiler and Erlang NIF headers:

```sh
nix develop
cargo test --workspace --locked
```

## Build And Run

```sh
nix develop
cargo run -p aspenc -- emit aspen.yaml > aspen_program.erl
cargo run -p aspenc -- build aspen.yaml --out-dir aspen-build
cargo run -p aspenc -- run aspen.yaml --timeout-ms 5000
# Run the checked-in two-module example:
cargo run -p aspenc -- run examples/hello --timeout-ms 5000
```

`emit` prints one generated Erlang module, `aspen_program`, without invoking
Erlang. `build` writes the generated module and runtime sources, then compiles
three BEAM modules and the native `aspen_syscall_nif.so` into the output directory.
Keep these artifacts together when deploying. Each build links the root package and its local dependencies into one whole
Aspen program; serialized separately compiled package artifacts are not yet supported. `erlc`, `erl`,
and a C compiler (`cc`, or the executable named by `CC`) must be on `PATH` for
`build` and `run`; the Erlang installation must include NIF headers. The development
shell supplies Erlang/OTP 28, including the process aliases used for replies.

`run` builds in a temporary directory and executes a fresh BEAM VM. After the
configured entry message has been sent, it waits for remaining actor work to drain and
unreachable actors to be collected before shutting down. Fire-and-forget sends
therefore finish even when the entrypoint has returned. An idle syscall singleton
does not prevent exit; an executing autonomous loop does. The runtime also exposes
a session API for Erlang hosts that need a longer-lived session.
Actor liveness collection reclaims unreachable idle actors and suspended calls,
including inactive cycles, while preserving executing or runnable autonomous
cycles. External handles and native I/O are retained conservatively. There is no
supervision, request failure reply, or implicit request timeout.
`--timeout-ms` sets an optional hard runtime deadline;
expiration fails the run, never reports successful completion. Compilation and VM
startup are outside this runtime deadline. Without the flag, a missing reply can
wait indefinitely.

## Compiler Debugging CLI

```sh
cargo run -p aspenc -- --help
cargo run -p aspenc -- lex example.aspen
cargo run -p aspenc -- parse example.aspen
cargo run -p aspenc -- check aspen.yaml
cargo run -p aspenc -- check --typed-ast aspen.yaml
cargo run -p aspenc -- lower aspen.yaml
printf 'export let greeting = "hello".' | cargo run -q -p aspenc -- parse -
```

- `lex` prints located tokens, including whitespace. It reports lexical errors
  while continuing to print the tokens it can recognize.
- `parse` prints the located AST only if parsing succeeds.
- `check` resolves and checks all package modules and dependencies, validates the
  configured entrypoint, and prints `ok` on success. `--typed-ast` instead prints
  the checked globals and entry send, including type evidence and bindings.
  Files with lexical or parse errors are not passed to the type checker.
- `lower` checks a program and prints the backend-independent executable IR.
  It does not execute the program or generate BEAM code. The dump exposes
  sequencing, message sends, captures, projections, and static adaptation plans.

`lex` and `parse` accept one UTF-8 source file; `-` reads standard input.
Compilation commands accept a package manifest or package directory. Debug output goes to
stdout, and diagnostics go to stderr as `file:line:column: error: message`, with
related source locations where available. Lines and Unicode scalar columns are
one-based. The type checker currently stops at the first type error.

Exit status is 0 for success, 1 for source, I/O, build, or runtime errors
(including runner timeout), and 2 for invalid CLI arguments. AST and token dumps use Rust debug formatting and are intended for
inspection, not as a stable machine-readable format.

## Statements And Replies

Method bodies contain zero or more period-terminated statements. The following
examples are method-body fragments, not standalone source files:

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

A signature such as `{put: {}. ready}` has no replies. `{ready -> {}}` promises
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
