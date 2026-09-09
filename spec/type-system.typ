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
recursive patterns is not
part of this fragment.

```text
expression ::= actor | identifier
             | "let" pattern "=" expression "." expression
pattern    ::= "_" | identifier
```

The first expression of a let is its initializer; the expression following the
dot is its body. Identifiers start with a Unicode alphabetic character or underscore and
continue with Unicode alphanumeric characters or underscores. The exact words
`let` and `def` are reserved; the exact word `_` is the discard pattern. An identifier is
permitted both as a variable-binding pattern and as a reference expression.
The notation in the
rules below describes compiler operations, not additional source syntax.

= Semantic Types and Subtyping

The initial three semantic types are:

- `never`: the bottom type, representing absence of a normally produced value.
- `{}`: the structural unit actor type, produced by the empty actor expression.
- `any`: the top type, accepting every type in the fragment.

On these three types, the subtype relation is the reflexive and transitive closure of:

```text
never <: {} <: any
```

Thus every type is a subtype of itself, `never` is a subtype of every type, and
every type is a subtype of `any`. In particular, `any` is not a subtype of `{}`,
and `{}` is not a subtype of `never`.

Nonempty actor structures and their subtyping rules are specified in the
Actor Receivers section. Types are semantic values: source locations do not
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

A variable pattern mints a fresh parameter `A <: any`, records its pattern
origin separately, and supplies accepted-type template `A` with binding template
`x : A`. The bound is an upper bound: admissible arguments are subtypes of `any`.
Preparation alone does not universally quantify the parameter. In a let,
checking and matching the initializer instantiates `A` with its precise actual
type and retains its evidence. For example, `let x = {} . {}` records `x` with type `{}`, not
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
plan = prepare(pattern)
initial = synth(Gamma, initializer)
instance = instantiate(plan, initial) // checks parameter bounds
bindings = instance.bindings
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

Actor comparisons may descend through method inputs and outputs. A mismatch
should retain the structural comparison path as well as available component
origins; root mismatches remain possible when a required method is absent.

For example, checking `{}` against `Expected(never, origin)` fails at the
root, reporting expected `never`, the supplied origin, actual `{}`, and the
empty actor's evidence. Checking a nested let against that same expectation
reports the nested result expression's evidence, not merely the outer let's
span. Pattern-driven initializer checks cannot themselves reject a
successfully synthesized type in this fragment, since discard accepts `any` and variable parameters have upper bound
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
syntax. Patterns currently bind only whole values; structural actor method
types are described next.

= Actor Receivers and Structural Actor Types

Actor expressions now contain zero or more period-separated methods:

```text
actor  ::= "{" (method ("." method)*)? "}"
method ::= "def" pattern "=>" expression
```

There is no trailing separator. The `def` keyword occurs only in expressions,
not in type notation. A let inside a method body consumes its own separating
period before parsing continues with the actor's method separator. For example:

```text
{ def x => let y = x. y. def _ => {} }
```

Receiver patterns supply accepted input types in the same way as let patterns.
Unlike a let initializer, there is no particular incoming value to narrow:
a variable receiver is bound to a fresh rigid parameter `A <: any`. All
parameters collected by the receiver plan are universally quantified at the
method level. Discard retains input type `any` without a parameter. Each method
body is typed in the enclosing lexical environment extended only with that
method's bindings. Sibling methods do not share receiver bindings.

```text
{ def x => x. def _ => {} }
```

Synthesizes the structural type:

```text
{ <A <: any> A -> A. any -> {} }
```

For each method, retain its source span, pattern expectation, bindings, and typed
body. Semantic method signatures contain parameter declarations, input and output types; source
locations remain separate. Preserve methods in source order without rejecting
or merging overlapping signatures.

== Structural Subtyping

The type universe now includes arbitrary finite actor structures whose method
inputs and outputs are themselves types, in addition to `never` and `any`.
The original three-type chain remains a sublattice, not the entire universe.
For monomorphic actor signatures the relation is:

```text
T1 <: T2 iff
  for every (I2 -> O2) in T2,
  there exists (I1 -> O1) in T1 such that
    I2 <: I1 and O1 <: O2
```

Inputs are contravariant and outputs covariant. Extra methods are permitted;
every actor type is a subtype of the empty actor type `{}`. An empty actor does
not satisfy a nonempty actor requirement. Bottom and top retain their universal
rules. Signature comparisons recurse through actor input and output types.

== Overlap Boundary

Overlapping and shadowing receivers are accepted. This subtype rule describes
structural capabilities, not an ordered runtime dispatch algorithm. No send or
dispatch operation exists yet. Before adding one, dispatch and any progressive
input narrowing must ensure that a shadowed signature cannot promise a result
inconsistent with the receiver actually invoked.

== Current Diagnostic Representation

Receiver input evidence is explicitly marked as a receiver-pattern origin,
not an expression origin. A method body referencing its receiver retains both
the reference expression and that input origin. Actor evidence stores each
method's accepted-input expectation and output evidence; aliases link back to
this structure rather than discarding its components.

On a failed actor comparison, the diagnostic records the first unsatisfied
expected method. If actual methods exist, it additionally records a
representative candidate and the failing input or output path, recursively.
For quantified signatures, the current path stops at the signature comparison;
the complete parameter and method evidence remains available rather than
reporting a misleading uninstantiated component comparison.
The candidate is not a dispatch choice, nor does its failure alone prove the
actor mismatch: subtyping first checks that no actual method satisfies the
requirement. The complete actual evidence and enclosing expected origin remain
available. Recursive expected-pattern component origins are still deferred
until recursive patterns exist.


= Implicit Bounded Method Polymorphism

Pattern preparation returns a parameter collection, an accepted-type template,
a binding template, and source provenance. Parameter identities are fresh even
when source names or spans coincide. Display names such as `A` and `B` are not
semantic identities. Semantic types contain no locations.

```text
prepare(_) = ([], any, discard)
prepare(x) = ([A <: any], A, bind x : A)  // A fresh
```

== Let Instantiation

A let has an actual value. Instantiation checks that value's type against the
parameter's upper bound, then substitutes the actual type for the parameter.

```text
let x = {}. x
A <: any; actual = {}; check {} <: any; substitute A := {}; bind x : {}
```

The inequality `{} <: A <: any` alone would also allow `A = any`. The binding
operation deliberately selects the precise checked actual type instead. A let
records each parameter declaration together with the actual type evidence that
instantiated it; bindings and these instantiation records share that evidence.
A let inside a generic receiver can instantiate its parameter with an enclosing
rigid parameter. It neither widens that parameter to its bound nor quantifies it
again.

== Receiver Quantification

A receiver has no particular initializer. Its parameters remain rigid while its
body is checked. All parameters introduced by that receiver pattern are bound
by a universal quantifier over the entire signature, including nested outputs.

```text
{ def x => x } : { <A <: any> A -> A }
{ def x => let y = x. y } : { <A <: any> A -> A }
{ def x => { def y => x } } : { <A <: any> A -> { <B <: any> B -> A } }
```

An upper bound justifies `A <: any`, not `A <: {}`. Body checking cannot solve
rigid `A` to a convenient concrete type. Inner methods quantify only their own
pattern parameters; captured outer parameters remain free relative to the inner
signature and bound by the enclosing signature.

== Quantified Signature Comparison

Actor comparison still requires every expected method to have a compatible
provided method. Expected parameters are freshened to rigid variables before
comparison. A provided generic method may instantiate its parameters, subject
to their bounds, to satisfy that expected signature. Input contravariance and
output covariance then apply to the instantiated signature. Bound parameters
are renamed without capturing enclosing variables; spelling is irrelevant.

```text
{ <A <: any> A -> A } <: { {} -> {} }
{ any -> any } is not a subtype of { <A <: any> A -> A }
```

The implemented instantiation rule targets the currently generated receiver
form: a single parameter occupying the whole input. That parameter is
instantiated with the expected input, checked against its upper bound, and
substituted throughout the output, including nested actor structures. General
constraint solving for future recursive-pattern inputs is not implemented.
Alpha-renamed compatible signatures and monomorphic comparisons remain
supported. Overlap/dispatch restrictions described earlier still apply.

== Recursive Pattern Extension Contract

Future recursive patterns prepare child plans independently and concatenate
their parameter collections into one enclosing plan. For a hypothetical pair
pattern `(x, y)`, this gives parameters `A <: any, B <: any`, accepted template
`Pair<A, B>`, and component binding templates `x : A, y : B`. A receiver collects
both parameters into its method quantifier, never separate component quantifiers.
A let instead matches actual components and records their respective type
evidence and pattern origins. No pair syntax or recursive pattern matching is
implemented yet. Each new pattern must define its matching/instantiation rule;
arbitrary subtype constraints need not have a unique most precise solution.
