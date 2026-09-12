# Modules And Packages

A source file defines one module. A compilation unit contains all modules in one
package; a program links those units and selects a global actor to receive an
initial atomic message. There is no script-style top-level statement sequence.

## Package Layout

A package uses an `aspen.yaml` manifest:

```yaml
name: hello
source: src
entry:
  actor: hello/main
  message: start
```

With this manifest, `src/index.aspen` defines module `hello`,
`src/sub/index.aspen` defines `hello/sub`, and `src/sub/tool.aspen` defines
`hello/sub/tool`. The source root is relative to the manifest. Module names do
not include the `.aspen` extension or a terminal `index` component. Defining
both `src/sub.aspen` and `src/sub/index.aspen` is an error.

```aspen
export let main = {
  def start => syscall write: 1 data: "Hello, world!\n".
}.
```

The entry actor must be exported and accept the configured atomic message with
no reply. Execution uses an ordinary send, then the existing runtime waits for
actor work to drain. Global declarations do not implicitly execute any method.

Local dependencies name other packages:

```yaml
dependencies:
  helpers: ../helpers
```

Dependency paths are relative to the referring manifest. Each dependency has its
own package manifest and source root. Imports do not instantiate another copy of
a package or its globals.

## Imports And Visibility

```aspen
import package_name/sub/module.
import package_name/sub as aliased_name.
import package_name/other (global, other_global as alias).

let private_global = 123.
export let public_global = "hello".
```

The first form binds `module`; the second binds `aliased_name`; the third binds
only the selected globals under their local names. Imports can access exported
declarations only, even within the same compilation unit. Imports and global
declarations are order-independent. Duplicate module or declaration bindings
are errors, not source-order shadowing.

A qualified reference such as `aliased_name/public_global` is a statically
resolved name, not a runtime message or module value. Slash spacing matters:

```aspen
tools/output       // qualified global reference
tools / output     // operator message send
```

Partially spaced paths are rejected. Every slash in a qualified reference must
be adjacent to both names. Local method bindings remain lexical and may shadow
global value bindings. The existing shadowable `syscall` fallback is unchanged.

## Type Aliases

Modules can declare transparent type aliases, with optional explicit parameters:

```aspen
type Number int.
export type Box<T> #box: T.
export type Readable { read -> Number }.
```

Aliases are order-independent and private unless exported. Import them explicitly
with `import package_name/types (type Box as LocalBox, value).`, or use a module
import and write `types/Box<int>`. A value and a type may have the same name;
selective imports without `type` select only values. Duplicate bindings within
either namespace are errors. Qualified type paths require adjacent slashes just
like value paths.

An exported alias can reference private helper aliases in its defining module;
importers do not gain access to those private names. Alias definitions are checked
even when unused. An alias does not create a runtime value or a nominal type:
`Box<int>` is the same structural type as `#box: int`. Type parameters shadow
module type names within their own bounds, annotations, and nested method bodies.

## Declarative Globals

Each global denotes one shared value per running program. Global initializers
allow primitive literals, global references, actor literals, and selectors with
recursively declarative payloads. Parentheses group these forms. Sends, local
statement sequences, and reply-target references cannot initialize globals.
Actor bodies are executable method bodies and are not subject to this
initializer restriction.

References preserve actor identity. Aliasing a global actor or placing it in a
selector does not spawn another actor. Distinct actor literals establish
distinct actor identities, including literals nested in global selectors.
Actor literals evaluated later inside methods still create fresh actors.

Forward references are allowed. Construction dependencies must be acyclic when
references inside actor bodies are excluded. Thus mutually referring actor
behaviors are valid, but aliases `a = b` and `b = a`, or recursively expanding
selector values, are rejected.

## Checking And Linking

Modules are resolved before type checking. The compiler builds the module
reference graph, condenses strongly connected components into a DAG, and checks
components in dependency order. Mutually dependent modules are checked together;
module boundaries alone do not impose annotation requirements.

Actor method signatures come from receiver patterns and explicit reply
annotations, so their bodies can be checked after the signatures of recursive
global actors are available. Global bindings may use existing type-annotation
syntax, but must bind one name rather than a destructuring pattern.

Methods can declare explicit type parameters, and global annotations can contain
polymorphic structural signatures:

```aspen
export let { <T> do: T -> T } identity = {
  def <T> do: T value -> T => ^ value.
}.
```

Type parameters are local to their method or structural signature, rather than
module exports. Bounds default to `{}` and see the entire parameter list,
including forward references. Guarded recursive bounds such as
`<T <: { next -> T }>` are subtype constraints (F-bounds), not recursive type
equalities. Bare variable-bound cycles are rejected. Importers call polymorphic
methods with ordinary messages; type arguments are inferred rather than written
at the call site.

The initial implementation links local dependency sources into one generated
BEAM program. A stable serialized interface/object format and a remote package
registry are not part of this change.

Diagnostics retain source-file provenance across imports. Internally, linked
sources use disjoint line ranges with a source map. The current compact `u16`
position representation limits a linked program to 65,535 allocated source lines
(including the synthetic entry message); exceeding this limit is diagnosed, not
wrapped. Each file occupies at least one line. The checked package exposes a
`SourceMap` to resolve internal positions to file-local spans. AST/IR debug dumps
expose internal positions rather than a stable source metadata format.

Global storage is not by itself a reason to keep an idle actor alive forever.
The runtime must retain global actors reachable from live behavior and values,
while allowing unused idle globals and inactive cycles to collect.
