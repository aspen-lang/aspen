#set document(title: "Aspen Type System")
#set page(paper: "a4", margin: 24mm)
#set text(size: 11pt)
#set heading(numbering: "1.")

#align(center)[
  #text(size: 24pt, weight: "bold")[Aspen Type System]

  The current language fragment
]

= Scope and Syntax

This specification describes selectors, recursive patterns, structural actors,
message sends, implicit bounded method polymorphism, bidirectional typing, and
diagnostic provenance. It specifies parsing and static typing, not a runtime
implementation or an evaluation protocol.

```text
actor  ::= "{" (method ("." method)*)? "}"
method ::= "def" receiver-pattern "=>" expression
let    ::= "let" pattern "=" expression "." expression
```

There is no trailing method separator. A let consumes its own separating period
before parsing resumes at the enclosing actor's method separator. Identifiers
start with a Unicode alphabetic character or underscore and continue with
Unicode alphanumeric characters or underscores. The exact words `let` and `def`
are reserved; the exact word `_` is the discard pattern.

== Ordinary and Selector Modes

Expressions, patterns, and types share selector syntax. Ordinary mode interprets
an identifier as a variable reference, variable-binding pattern, or type name,
respectively. A `#` enters selector mode. Parentheses inside selector mode escape
to ordinary mode; parentheses in ordinary mode group ordinary syntax. After the
parenthesized form, parsing resumes in the enclosing mode.

Selector mode has three distinct forms:

```text
atomic   ::= symbol
operator ::= ("+" | "-" | "*" | "/") payload
keyword  ::= (label ":" payload)+
```

An atomic selector has a name and no payload. An operator selector has one of
the four operator tags and exactly one payload. A keyword selector has one or
more ordered label/payload pairs. Both the number and order of labels matter.
Payloads use the corresponding expression, pattern, or type syntax in selector
mode; parentheses allow an ordinary value, binding, or type as a payload.

```text
#ready                   // atomic selector expression
#+ ({})                  // operator selector with an actor payload
#put: ({}) at: (#home)    // keyword selector with two payloads
#outer: inner: (x)       // nested selector pattern, binding x
```

A `def` receiver pattern starts in selector mode. A let pattern starts in
ordinary mode. Thus `def x => ...` matches the atomic selector named `x` and
introduces no variable; `def (x) => x` binds a whole incoming value. Likewise,
`let x = ...` binds a variable, while `let #x = ...` requires an atomic selector.
Use `def (_) => ...` for an ordinary discard receiver.

Actor type inputs start in selector mode and outputs in ordinary mode. Types
outside actor inputs start in ordinary mode. The `def` keyword is not part of
type notation:

```text
{ ready -> {}. put: ({}) -> #done }
{ <A <: any> (A) -> A }
{ <A <: any, B <: any> put: (A) at: (B) -> A }
```

The parameter notation describes semantic signatures. Receiver variables
introduce those parameters implicitly; source patterns need no annotations.

== Message Sends and Precedence

A send is juxtaposition: the callee is parsed in ordinary mode and its message
in selector mode. Parentheses can therefore send an ordinary value as well as
group a callee or a payload.

```text
service ready
identity ({})
service put: (value) at: (#home)
```

Atomic and grouped-message sends bind tightest and associate left. Operator
sends using `*` and `/` bind more tightly than those using `+` and `-`; each tier
associates left. Keyword sends bind weakest. A keyword payload cannot consume
an unparenthesized keyword send: the following label belongs to the enclosing
keyword message. Parenthesize a keyword send used as a payload.

```text
a first second          // (a first) second
a + (b) * (c)           // a + ((b) * (c))
a put: (b get: (c))      // grouped keyword send as a payload
```

Selector construction and sending are distinct: `#ready` produces a selector;
`service ready` sends one. These precedence rules govern sends, not a numeric
arithmetic implementation.

= Semantic Types and Subtyping

Types are semantic values, independent of source locations:

- `never` is bottom, representing absence of a normally produced value.
- `any` is top, accepting every type.
- Actor types contain finite collections of quantified input/output signatures.
  The empty actor type is `{}`.
- Selector types contain an atomic tag, an operator tag and payload type, or an
  ordered nonempty sequence of keyword labels and payload types.
- A type variable has a fresh identity and an upper bound.

Every well-formed type is a subtype of itself; `never` is a subtype of every
such type and every such type is a subtype of `any`. Every actor type is a
subtype of `{}`, so `never <: {} <: any` remains valid, but does not describe
the whole universe. Selectors are not actors: `#ready` is not a subtype of `{}`.

== Selector Subtyping

Selector subtyping requires identical shape: the same variant and atomic name,
operator tag, or complete ordered keyword-label sequence. Corresponding payload
types are compared covariantly and recursively. There is no keyword width
subtyping, label reordering, or conversion between selector variants.

```text
#ready <: #ready
#put: ({}) <: #put: (any)
#outer: inner: ({}) <: #outer: inner: (any)
```

Different atomic names, different operators, or different keyword shapes do
not subtype one another. Top and bottom retain their universal rules.

== Structural Actor Subtyping

For monomorphic signatures:

```text
T1 <: T2 iff
  for every (I2 -> O2) in T2,
  there exists (I1 -> O1) in T1 such that
    I2 <: I1 and O1 <: O2
```

Inputs are contravariant and outputs covariant. Extra methods are permitted;
an empty actor does not satisfy a nonempty actor requirement. Comparisons
recurse through selector payloads and actor signatures. Quantified signature
comparison is specified later. Actor types must satisfy the disjoint-input
invariant; overlapping method collections are not valid actor types.

A rigid variable is a subtype of its upper bound and that bound's supertypes.
Sharing a bound does not identify two fresh variables or make them subtypes of
one another. In particular, `A <: any` does not justify `A <: {}`.

= Locations and Evidence

A located syntax node pairs syntax with its source span. Locations do not enter
semantic type identity. Typing carries two distinct forms of provenance:

```text
Expected(T, origin)
Actual(T, evidence)
```

The expected origin explains why a type is required, such as a receiver or let
pattern and its recursive components. Actual evidence explains where a result
type arose, which need not be the whole expression's span. A binding keeps the
checked actual type and evidence separately from its declaration origin.

An actor's evidence stores method input expectations and body output evidence.
Receiver input evidence is a pattern origin, not a fictitious expression.
Selector evidence stores corresponding payload evidence recursively. References
retain their use site and link to binding evidence; aliases must not discard
that component structure. A normally returning let's result evidence is its
body's result evidence, recursively through nested lets. An empty actor's
evidence identifies that actor expression.

= Typing Interface

An explicit lexical environment `Gamma` maps names to binding information:

```text
synth(Gamma, expression) -> Actual(T, evidence)
check(Gamma, expression, Expected(U, origin)) -> Actual(T, evidence)
```

Either operation may fail. Successful checking establishes `T <: U` and returns
the actual type `T`, not the expected type `U`. The fallback rule synthesizes,
then checks subtyping, with no conversion or implicit widening. For example,
checking `{}` against `any` still returns `{}` and its evidence.

Checking a normally returning let forwards the expectation and its origin into
the body. When the initializer has type `never`, synthesize the body without
that enclosing expectation and check the final `never` result instead. Public
convenience operations start with an empty environment; environment-aware
operations can accept caller-supplied bindings.

= Recursive Patterns and Binding Plans

Preparation produces fresh parameter declarations, an accepted-type template,
a recursive binding plan, and component origins. Preparation alone does not
quantify the parameters.

```text
prepare(_) = ([], any, discard)
prepare(x) = ([A <: any], A, bind x : A)  // A fresh
prepare(selector(children)) =
  (concatenate child parameters,
   selector(child accepted types),
   selector(child plans))
```

An atomic selector plan has no children, parameters, or bindings. Discard
accepts `any` without a binding. Each variable introduces its own fresh parameter
and binds at that position. Display names such as `A` and `B` are not semantic
identities, even when source names or spans coincide. Duplicate binding names
within one pattern are rejected; a new lexical scope may shadow outer names.

Matching checks selector variant, tag, arity, and ordered labels, then recursively
matches corresponding payloads. Each child receives its own actual type and
available component evidence, paired with that child's expected origin. Plans
must not widen every binding to the enclosing expectation or replace all child
provenance with the enclosing expression's span.

== Let Instantiation

A let has a particular initializer. At each variable leaf, check the actual
component against the parameter's upper bound, then instantiate the parameter
with that precise actual type. Record the parameter declaration and the evidence
that instantiated it; the binding shares this evidence.

```text
let #put: (x) at: (y) = #put: ({}) at: (#home). x
```

This binds `x : {}` and `y : #home`, with their respective payload evidence, and
returns `{}`. A mismatched label or nested selector shape is a pattern mismatch,
not a successful binding. A whole-value variable remains precise too:

```text
let x = {}. x
A <: any; actual = {}; check {} <: any; substitute A := {}
```

The inequality `{} <: A <: any` alone would also allow `A = any`. Instantiation
deliberately chooses `{}` instead. A let inside a generic method can instantiate
its fresh parameter with an enclosing rigid parameter, without widening that
parameter to its bound or quantifying it again.

== Receiver Quantification

A receiver has no particular initializer. Its variables remain rigid while its
body is checked. All parameters collected from the complete recursive pattern
are universally quantified together over the method input and output, including
nested outputs. There are no separate payload-level quantifiers.

```text
{ def (x) => x }
  : { <A <: any> (A) -> A }
{ def put: (x) at: (y) => x }
  : { <A <: any, B <: any> put: (A) at: (B) -> A }
{ def outer: inner: (x) => x }
  : { <A <: any> outer: inner: (A) -> A }
{ def (x) => { def (y) => x } }
  : { <A <: any> (A) -> { <B <: any> (B) -> A } }
```

Body checking cannot solve a rigid parameter to a convenient concrete type.
Each method body sees the enclosing environment plus only its own receiver
bindings. Sibling methods do not share bindings. Inner methods quantify only
their own parameters; captured outer parameters remain bound by the enclosing
signature.

= Expression Rules

== Actors and References

The expression `{}` synthesizes the empty structural actor type with evidence
at those braces. Braces produce an actor, not a type annotation or a declaration
block. For a nonempty actor, prepare each receiver, type its body under its rigid
bindings, collect the quantified signature and method evidence, and validate
pairwise input disjointness before accepting the actor. Preserve source order
for evidence and representation, not to give earlier methods priority.

A reference resolves to the nearest enclosing binding and synthesizes its type
with reference evidence linked to the bound value and declaration. An absent
name is an unbound-variable error retaining the name and use span. Checking a
reference preserves this evidence.

== Selector Expressions

An atomic selector synthesizes its atomic selector type. Operator and keyword
selectors synthesize each payload and form a structural selector type from the
resulting actual payload types, retaining their recursive evidence. Construction
does not look up a receiver or widen payloads to expected types. All payloads
are statically typed. If a payload has type `never`, the entire selector
expression has type `never` rather than promising a normally produced selector.

== Let

For `let pattern = initializer . body`:

```text
plan = prepare(pattern)
initial = synth(Gamma, initializer)
instance = instantiate(plan, initial)
result = synth(Gamma extended with instance.bindings, body)
if initial.type == never:
    return Actual(never, initial.evidence)
else:
    return result
```

The new bindings are visible only in the body, not in their own initializer,
and do not leak out of the let. Let is strict in its initializer. Bottom can
match a recursive plan without a normally produced selector; binding leaves
receive bottom information for the unreachable body. The body is still typed:
strictness does not license skipping static checks. Otherwise the let returns
exactly its body's actual type and evidence.

== Sends

Synthesize both callee and message without using an expected result to infer
method parameters. Expose a callee variable's upper bound when looking for its
actor capabilities. If the callee or message is `never`, the send synthesizes
`never`; both operands are still statically typed.

For a normally returning send, the callee must have a valid actor type. For
each signature, infer parameter instances recursively from the message type
and the input template, check upper bounds, and test that the message is a
subtype of the instantiated input. A signature passing these tests is applicable.
Disjoint receiver domains ensure at most one applicable receiver for an inhabited
message. The send returns that receiver's instantiated output type and retains
the callee, message, and selected method in its typed representation.

A non-actor callee is a not-an-actor error. An actor with no applicable signature
is a no-receiver error retaining both operands' evidence. An overlapping actor
type supplied externally is invalid, not an instruction to pick the first
receiver. No runtime delivery or execution is implemented by these typing rules.

```text
{ def ready => {} } ready                 // {}
{ def (x) => x } (#home)                  // #home
{ def put: (x) at: (y) => x } put: ({}) at: (#home)
                                          // {}
```

= Disjoint Receiver Inputs

Every pair of receiver inputs must be provably disjoint at actor construction.
For a generic signature, its accepted domain replaces method parameters with
their upper bounds, recursively and with earlier substitutions applied to later
bounds. Thus two independently fresh whole-value parameters bounded by `any`
are overlapping, not distinct dispatch tags.

The conservative disjointness proof uses these rules:

- `never` is disjoint from every type.
- Variables are examined through their upper bounds.
- Actor and selector types are disjoint.
- Selectors with distinct variants or shapes are disjoint.
- Selectors with matching shape are disjoint if at least one corresponding
  payload pair is provably disjoint, recursively.
- Actor versus actor is conservatively unknown, even for different method sets.
  Other unproved cases, including ordinary overlap with `any`, are not disjoint.

Failure to prove disjointness rejects the actor with both receiver origins.
This is stricter than merely rejecting identical signatures: neither ordering,
shadowing, nor result-type differences resolve overlapping domains.

```text
{ def ready => {}. def stop => {} }            // accepted
{ def put: left => {}. def put: right => {} }  // accepted
{ def (x) => x. def ready => {} }              // rejected
{ def put: (x) => x. def put: (y) => {} }      // rejected
```

The first two actors separate messages by atomic tags, including nested tags.
The latter two overlap after replacing variable inputs by their upper bounds.
Well-formed actor type notation obeys the same invariant as actor expressions.

= Bounded Instantiation and Signature Comparison

Input-directed instantiation supports the receiver forms generated by recursive
selector patterns, not arbitrary constraint solving. A method parameter at an
input leaf is instantiated with the corresponding actual message type. Matching
selector structure recursively exposes those leaves. A captured message variable
can expose selector structure through its upper bound. Check the inferred
instances against substituted bounds, substitute into the entire input, and
finally verify message subtyping. Parameters not exposed by a bottom input may
fall back to their bounds. Substitute successful instances throughout the output,
including selector payloads and nested actor signatures.

For actor subtyping, every expected signature must have a compatible provided
signature. Freshen expected parameters to rigid variables. Instantiate only the
provided method's parameters, input-directed by that freshened expected input,
and check bounds. Input contravariance and output covariance then apply to the
instantiated signature. Compatible alpha-renamed signatures are supported.

```text
{ <A <: any> (A) -> A } <: { ({}) -> {} }
{ <A <: any> put: (A) -> A } <: { put: ({}) -> {} }
{ (any) -> any } is not a subtype of { <A <: any> (A) -> A }
```

Substitution is simultaneous and capture-avoiding. Locally quantified parameters
are renamed before descending into nested signatures; captured outer parameters
are substituted without capturing them under an inner quantifier. Parameter
spelling is irrelevant. An upper bound restricts admissible instances; it neither
identifies distinct variables nor permits solving rigid expected parameters.
General inference for arbitrary recursive constraints beyond the supported
selector input structure is not implied.

= Diagnostics

A failed subtype or pattern check retains the expected semantic type and origin,
the actual semantic type and evidence, and the structural disagreement path.
Paths may descend through selector payloads and actor method inputs and outputs.
A shape mismatch can stop at the selector itself; a missing required method can
remain a root actor mismatch. Available child origins and actual component
evidence remain attached rather than being replaced by coarse enclosing spans.

For a failed actor comparison, record the first unsatisfied expected method and,
when available, a representative actual candidate and its failing input or output
path. Subtyping first checks that no actual method satisfies the requirement;
a representative candidate is diagnostic context, not a dispatch selection.
For quantified comparisons, a path may stop at the signature while retaining
complete parameter and method evidence, rather than reporting a misleading
uninstantiated component comparison.

Checking `{}` against `Expected(never, origin)` reports a root mismatch with that
origin and the empty actor's evidence. Checking a nested normally returning let
against the same expectation reports its producing body's evidence. Recursive
selector patterns can themselves reject initializers, unlike unconstrained
variable and discard patterns. Overlapping-receiver, not-an-actor, and
no-receiver errors are distinct from an ordinary expected/actual mismatch.

Caller-supplied environments may supply `never`, `any`, or bounded variables.
The bottom rules do not claim a closed expression can construct a bottom value,
and checking against `any` does not make the expression synthesize `any`.
