# BEAM runtime direction and compiler foundations

Status: agreed design; compiler foundations only. No BEAM execution is implemented.
This document extends the static language specification with the intended runtime
contract. It does not claim a mechanized type-safety or erasure proof.

## Execution contract

A Program is compiled as one executable unit. Expressions evaluate strictly:
callee first, selector payloads in written order, then delivery. Statements run
in order. Each actor handles one request at a time. A replying send waits for its
first reply, including when its result is discarded in statement position. A
no-reply send enqueues its message and continues without waiting for handler
completion. Blocking suspends the BEAM process, not a scheduler thread.

Declared reply types constrain values, not delivery or cardinality. A receiver's
`^` is an ordinary actor value. Sending to it does not exit the handler. Finishing
a handler does not implicitly reply or invalidate its reply address. Passing `^`
as a payload delegates work by ordinary actor passing, not exclusive ownership
transfer. A delegate can reply after the originating handler has finished.

A value-producing send accepts its first reply. Later replies are discarded for
now; changing that policy to actor failure is explicitly deferred. There are no
implicit timeouts or inferred request failures. In particular, monitoring the
original receiver cannot determine failure after a reply target has been passed
to another actor. Supervision, cancellation, and deadlines are future features.

While waiting for a correlated reply, selective receive leaves ordinary requests
queued. At dispatch time an actor handles ordinary requests in mailbox order.
Sequential sends from one actor to the same destination preserve BEAM ordering;
there is no global order across senders or completion ordering through delegates.
Unmatched requests fail the receiving actor with a diagnostic rather than being
silently discarded or left permanently in its mailbox. Duplicate replies are a
separate protocol case, not unmatched ordinary requests.

Every evaluation of an actor expression, including `{}`, creates a fresh process
and endpoint with lexical captures. Actors can escape their creator and live until
termination, failure, or explicit runtime-session shutdown. Automatic reclamation
(possibly reference counting) is deferred. Top-level completion is not global
completion: delegated work may remain. The runner will support explicit shutdown
and a configurable hard timeout, reported as timeout rather than successful
completion. Integration tests will use completion signals and mandatory deadlines.

## Value and transport representation

Ordinary actors use PIDs. Reply actors use opaque correlated addresses backed by
BEAM process aliases; this avoids a dedicated process per request. Deactivate an
alias after the first accepted reply and explicitly discard duplicates already
queued. Alias deactivation alone does not remove queued messages. The backend must
choose a supported OTP version and test these races before claiming this protocol
implemented.

Identity belongs to the addressed endpoint. Distinct invocation reply addresses
are distinct even when they target the same PID. Aliasing and structural views
preserve identity. Actor-kind dispatch recognizes both ordinary and reply handles;
it cannot be implemented solely as `is_pid`.

Selector terms are intended to use these shapes:

```erlang
{aspen_atomic, ready}
{aspen_plus_operator, Payload}
{aspen_minus_operator, Payload}
{aspen_star_operator, Payload}
{aspen_slash_operator, Payload}
{aspen_keyword, {put, at}, {Value, Location}}
```

Nested selectors use the same representation recursively. Labels retain their
order and arity. Reserved actor-handle tags cannot collide with selector tags.
Only compiler-controlled source names become atoms; arbitrary runtime strings do
not create atoms. Transport request/reply envelopes are distinct from Aspen
selector payloads. Exact layouts are an internal ABI, not source semantics.

## Dispatch and structural adaptation

Dispatch operates on the full message, never a static interface method index.
The current disjointness rules inspect selector structure recursively, actor vs
selector kind, bounds, and empty domains. They do not distinguish two structural
actor interfaces. A whole-value receiver is supported, including one broad
provided signature satisfying multiple required signatures.

Runtime shapes abstract types as follows:

- `never`, and a selector containing an empty payload: empty.
- `any`: wildcard.
- variable: recursively abstract its upper bound.
- actor interface: opaque actor-kind, regardless of its method collection.
- selector: retain variant, tag, ordered labels and recursively abstract payloads.

These predicates select handlers for well-typed values. They are not runtime type
validators: matching actor-kind does not prove a particular actor interface.
Every constructed actor must have pairwise-disjoint erased input shapes.

Subsumption elaborates into a recursive adaptation plan using the existing subtype
algorithm. For S exposed as T, each required signature has a compatible provided
signature. Inputs adapt from required to provided (contravariant); replies adapt
from provided to required (covariant). No-reply and replying signatures remain
incompatible. Selector payload obligations descend covariantly; nested actor
obligations include future sends and replies, not merely the current payload.
Expected method parameters remain rigid while provided parameters are instantiated
using the existing supported input-directed inference.

Plans are static evidence. Method correspondence in a plan is not a physical
runtime slot mapping. Under the uniform ABI all supported plans are expected to
reduce to identity. The first implementation supports only identity lowering and
must report unsupported adaptation rather than emit an unsafe call. General
runtime adapters, shared witnesses, and their composition are deferred until a
concrete representation-changing feature requires them. The special correlated
reply-handle representation is not itself a structural adapter.

## Erasure argument and limits

The proof obligations for the current finite fragment are:

1. Shape abstraction preserves static disjointness. Each checker rule either
   distinguishes retained constructors, descends to a distinguished payload, or
   eliminates an empty domain. Actor interfaces are never used as distinct tags.
2. A valid value inhabiting a type matches its abstract shape. This depends on
   typed construction and the chosen uniform actor/selector representation.
3. For an inhabited call through T, subtype elaboration supplies a receiver in S
   accepting the unchanged message. Concrete erased receiver shapes are disjoint,
   so no competing receiver can capture that message.
4. Input and reply obligations recursively preserve representation, including
   actors passed inside selectors and reply targets passed as ordinary actors.
   Generic code is uniform: fresh static variable identities do not become tags.
5. Executable lowering preserves lexical binding, capture, strict evaluation
   order, reply mode, and full-message dispatch.

The implementation and tests support this argument but are not a formal proof.
True recursive types, arbitrary polymorphic constraint solving, external untyped
values, representation-changing features, and mixed ABIs are outside its scope.
Current type trees are finite; recursive descent is not a coinductive algorithm.
Bottom denotes no normally produced value, not a runtime message representation.

## Foundation inspection

`aspenc check --typed-ast FILE` exposes static adaptation evidence alongside the
existing diagnostic evidence. `aspenc lower FILE` emits the executable IR in debug
form. Neither command runs a program. Dumps are inspection tools, not stable
package metadata formats.

The IR retains static `Check` and send-adaptation metadata for inspection; these
are not runtime type checks or wrapper allocation. Runtime operations use lexical
binding/value IDs, explicit capture lists and payload projection paths, full
message sends with wait-first/no-reply modes, and erased receiver shapes. Source
spans support diagnostics without making provenance pointers executable identity.
A future emitter must omit proof metadata and empty receiver clauses.

## Compiler milestones

1. Extend subtype checking with inspectable adaptation plans; preserve executable
   recursive pattern information; add runtime-shape validation and an explicitly
   sequenced IR with lexical IDs, captures, projections, and reply parameters.
2. Validate with structural and bounded exhaustive shape tests, IR regression
   tests, and this written argument. Do not build a separate reference runtime.
3. Generate Erlang source from the Aspen IR, compile with `erlc`, and implement a
   small runtime module for the agreed process/reply protocol and session runner.
4. Test real BEAM behavior: delegated replies, acknowledgments, alias races,
   duplicate cleanup, actor failure, ordering, and nested structural use.

Future separate compilation is package-level, not source-file-level. Packages
will export type metadata and an ABI contract. Consumers can derive subsumption
plans without concrete implementation knowledge. No package-local selector IDs,
physical interface slots, or concrete-callee specialization may be required for
correctness. Package metadata formats and nonidentity adapter linkage are deferred.
