#set document(title: "Aspen Type System")
#set page(paper: "a4", margin: 24mm)
#set text(size: 11pt)
#set heading(numbering: "1.")

#align(center)[
  #text(size: 24pt, weight: "bold")[Aspen Type System]

  The current language fragment
]

= Scope and Syntax

This specification describes the semantic types, bidirectional typing rules,
and diagnostic provenance of Aspen's current expression fragment. Syntax for
nonempty actors and recursive patterns is not
part of this fragment.

```text
expression ::= "{}" | identifier
             | "let" pattern "=" expression "." expression
pattern    ::= "_" | identifier
```

The first expression of a let is its initializer; the expression following the
dot is its body. Identifiers start with a Unicode alphabetic character or underscore and
continue with Unicode alphanumeric characters or underscores. The exact word
`let` is reserved; the exact word `_` is the discard pattern. An identifier is
permitted both as a variable-binding pattern and as a reference expression.
The notation in the
rules below describes compiler operations, not additional source syntax.

= Semantic Types and Subtyping

There are exactly three semantic types in this fragment:

- `never`: the bottom type, representing absence of a normally produced value.
- `{}`: the structural unit actor type, produced by the empty actor expression.
- `any`: the top type, accepting every type in the fragment.

The subtype relation is the reflexive and transitive closure of:

```text
never <: {} <: any
```

Thus every type is a subtype of itself, `never` is a subtype of every type, and
every type is a subtype of `any`. In particular, `any` is not a subtype of `{}`,
and `{}` is not a subtype of `never`.

The description of `{}` as structural does not introduce any rules for fields,
messages, members, or nonempty actors. None of those structures has typing
semantics here. The three types are semantic values: source locations do not
affect type equality or subtyping.

= Locations and Evidence

A located syntax node pairs syntax with its source span. A source-located type,
if used to report where a type was written or required, likewise keeps its
location separate from the underlying semantic type. Locations must not become
part of semantic type identity.

Typing carries two distinct forms of provenance:

- An expected type is paired with an origin explaining why it is required.
  When checking a let initializer, this origin is the corresponding pattern.
- An actual type is paired with evidence explaining where the expression's
  result type comes from. This need not be the span of the entire expression.

We write these pairs as `Expected(T, origin)` and `Actual(T, evidence)`.
Evidence is diagnostic information, not an extra semantic type constructor.
A binding stores the checked actual type and its evidence, as well as the
binding's own pattern origin. The declaration location and the evidence for
the bound value serve different purposes; neither replaces the other.

For `{}`, actual evidence identifies that empty actor expression. For a
normally returning let, actual evidence is the evidence of its body. This
propagation is recursive: if a body is itself a let, preserve its result's
evidence rather than replacing it with either enclosing let's span.

= Typing Interface

An environment `Gamma` maps in-scope variable names to binding information.
Typing is bidirectional:

```text
synth(Gamma, expression) -> Actual(T, evidence)
check(Gamma, expression, Expected(U, origin)) -> Actual(T, evidence)
```

Either operation may fail with a diagnostic. Successful checking establishes
`T <: U` and returns the synthesized actual type `T`, not the expected type
`U`. Checking against a wider type does not erase information. For example,
checking `{}` against `any` returns actual type `{}` with the empty actor's
evidence; it does not return `any`.

The fallback checking rule is synthesis followed by a subtype test:

```text
actual = synth(Gamma, expression)
if actual.type <: expected.type:
    return actual
else:
    fail Mismatch(expected, actual, root)
```

There is no conversion or implicit widening step in this rule. Checking a
normally returning let instead forwards the expectation, including its origin,
into the body. This is equivalent in accepted types for the current fragment
and locates a failure at the producing body expression. When the initializer
has type `never`, synthesize the body without the enclosing expectation and
check the final `never` result instead.

The implementation passes an explicit lexical environment through synthesis,
checking, and initializer checking. Each let extends an immutable outer scope
for its body only. The public convenience operations start with an empty
environment; environment-aware operations also accept caller-supplied bindings.

= Patterns and Bindings

A pattern supplies an expected type and describes the bindings to install
once its initializer has been checked.

== Discard Pattern

The pattern `_` requires `Expected(any, pattern-origin)`. It accepts every
actual type and introduces no binding. Discarding a value does not remove its
initializer from typing or from the strictness rule for let.

== Variable Pattern

A variable pattern also requires `Expected(any, pattern-origin)`. After
checking the initializer, it binds its name to the returned actual type and
provenance. For example, `let x = {} . {}` records `x` with type `{}`, not
`any`, and with evidence from the initializer's empty actor.

The binding is visible only while typing the body. It is not visible in its
own initializer and does not leak outside the let. A binding of an existing
name shadows that name in the body; leaving the body restores the outer
environment. A reference resolves to the nearest enclosing binding.

= Expression Rules

== Empty Actor

```text
synth(Gamma, {}) = Actual({}, evidence-at-this-expression)
```

The braces form an expression producing the structural unit actor type; they
are not a type annotation or a block of declarations.

== Variable Reference

```text
binding = lookup(Gamma, name)
synth(Gamma, name) = Actual(binding.type, reference-evidence)
```

Lookup selects the innermost binding. An absent name is an unbound-variable
error retaining the name and reference span. Reference evidence records the
use site and links to the binding's value evidence. The typed reference also
retains the binding's pattern origin. Diagnostics can therefore distinguish
where a value was used, where its name was declared, and where its type arose.
Checking a reference uses the usual subtype test and preserves all this evidence.

== Let

For `let pattern = initializer . body`, use the following sequence:

```text
expected = Expected(any, pattern-origin)
initial = check(Gamma, initializer, expected)
bindings = bind(pattern, initial)
result = synth(Gamma extended with bindings, body)
if initial.type == never:
    return Actual(never, initial.evidence)
else:
    return result
```

Here `bind(_, initial)` is empty. For a variable pattern, `bind` records the
name, the checked actual type and evidence, and the pattern origin. The
extended environment is lexical and is used only for the body.

Let is strict in its initializer. If the initializer has type `never`, the
whole let has type `never`, irrespective of the body's type. The evidence in
that case comes from the initializer that prevents normal completion. The
body is still typed under the extended environment; the strictness rule is
not a license to omit its static checks.

Otherwise, the let returns exactly the body's actual type and evidence.
Checking the whole let against an expected type tests this final result, not
the initializer's type. Any enclosing expected origin remains available if
that check fails.

No closed current expression produces `never`. A caller-supplied environment
may provide a variable of type `never`. The strictness case nevertheless
specifies how the type checker treats such an initializer, without claiming
that the present syntax can construct one. Likewise, checking against `any`
does not create an expression whose synthesized type is `any`.

= Mismatch Diagnostics

A failed subtype check retains all of:

- The expected semantic type and its origin.
- The actual semantic type and its evidence.
- The structural path at which the types disagree.

The current types have no recursively checked components, so every mismatch
path is the root. Keeping a path explicitly does not imply that component
paths or recursive actor types are already supported.

For example, checking `{}` against `Expected(never, origin)` fails at the
root, reporting expected `never`, the supplied origin, actual `{}`, and the
empty actor's evidence. Checking a nested let against that same expectation
reports the nested result expression's evidence, not merely the outer let's
span. Pattern-driven initializer checks cannot themselves reject a
successfully synthesized type in this fragment, since both patterns expect
`any`; the general checking interface can still express narrower expectations.

= Worked Examples

```text
{}
```

Synthesizes `{}` with evidence at those braces.

```text
let _ = {} . {}
```

Checks the initializer against `any` with the discard pattern as expected
origin. It introduces no binding and synthesizes `{}` with evidence at the
body's braces.

```text
let x = let y = {} . {} . {}
```

The initializer of `x` is itself a let. Its result evidence comes from the
body of that inner let, and that evidence is retained in the binding for `x`.
The binding for `y` is scoped to the inner body, not to the body of `x`.
The whole expression's result evidence comes from the final empty actor.

```text
let x = {} . x
```

This synthesizes `{}`. The final reference resolves to `x`, retains its use
site, and links to the initializer evidence and the declaration pattern.

= Extension Boundary: Binding Plans

Future recursive patterns can generalize the existing pattern process into
a binding plan. Such a plan would pair an expected type and its origin with
instructions for extracting each binding from a successfully checked actual
value's type and evidence.

For the current patterns the plan is trivial: discard has no binding action,
and a variable binds the entire checked actual type with its evidence. A
future recursive plan would need to identify a component's structural path,
retain that component's actual type and evidence, and associate any failure
with the corresponding expected component origin. It must not widen all
bindings to the enclosing expected type or give every component the same
coarse enclosing-expression provenance.

This is an extension constraint, not a definition of recursive pattern
syntax, actor members, component subtyping, or implemented recursive evidence
structures. The current specification requires only whole-value bindings,
the three semantic types, and root mismatch paths.
