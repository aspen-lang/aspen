# BEAM runtime direction and compiler foundations

Status: initial Erlang-source backend and BEAM runtime implemented (OTP 28+).
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
termination, failure, liveness collection, or explicit runtime-session shutdown.
Collection preserves autonomous executing cycles; only actors without a possible
activation path can be reclaimed. Top-level completion is not global
completion: delegated work may remain. The runner will support explicit shutdown
and a configurable hard timeout, reported as timeout rather than successful
completion. Integration tests will use completion signals and mandatory deadlines.

## Value and transport representation

Every value is an actor in the source type system, including primitives. Runtime
representation distinguishes process-backed actors from primitive values without
introducing a separate source-language actor kind. The unit actor type `{}` is
top; it does not imply that a value has a process handle or any receivers.

Process-backed actors use PIDs. Reply actors use opaque correlated addresses backed by
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

`bytes` values are immutable arbitrary octet sequences represented as BEAM
binaries. `string <: bytes`: strings refine this representation with a valid-UTF-8
guarantee, rather than using a distinct runtime tag. String literals are decoded
as Unicode text and emitted as UTF-8 binaries. Generated Erlang uses numeric
binary bytes rather than quoted Erlang strings, so escaping and source-file
encoding cannot change their contents. Passing a string to a `bytes` parameter
requires no encoding or copy. There is no byte-literal syntax yet.

Float literals are finite binary64 values and lower to Erlang floats. Generated
literals use round-tripping scientific notation with an explicit decimal point,
including for integral values and signed zero. Subnormals are preserved; overflow
to infinity is rejected during lexing. Integer and float dispatch remain distinct.

## Dispatch and structural adaptation

Dispatch operates on the full message, never a static interface method index.
The current disjointness rules inspect primitive families, selector structure
recursively, bounds, and empty domains. A structural actor interface does not
prove disjointness from a primitive, nor do two structural actor interfaces
prove disjointness from one another. A whole-value receiver is supported, including one broad
provided signature satisfying multiple required signatures.

Runtime shapes abstract types as follows:

- `never`, and a selector containing an empty payload: empty.
- `{}`: wildcard, accepting primitive values as well as process-backed actors.
- variable: recursively abstract its upper bound.
- nonempty actor interface: opaque actor-handle predicate in the current runtime,
  where primitives expose no methods; method collections are not dispatch tags.
- `bytes`: any BEAM binary.
- `string`: a BEAM binary containing valid UTF-8.
- `int`, `float`: the respective primitive representation kind.
- `selector`, `atom`, `optagged`, `keywordtagged`: the corresponding selector
  family predicate, independent of concrete tags and payload types.
- structural selector: retain variant, tag, ordered labels and recursively
  abstract payloads.

These predicates select handlers for well-typed values. They are not runtime type
validators: matching an actor handle does not prove a particular actor interface.
The static disjointness proof remains conservative about primitives implementing
actor interfaces; it does not use actor handles as a separate source type.
Every constructed actor must have pairwise-disjoint erased input shapes.
`string` and `bytes` overlap, so they cannot distinguish separate receivers.
UTF-8 validity is checked after binary-shape matching, since validation is not
an Erlang guard BIF. A failed string refinement rejects the message; disjointness
ensures no other receiver could accept it. Valid UTF-8 bytes satisfy this runtime
refinement regardless of their static source type.

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

`aspenc check --typed-ast aspen.yaml` exposes static adaptation evidence alongside the
existing diagnostic evidence. `aspenc lower aspen.yaml` emits the executable IR in debug
form. Neither command runs a program. Dumps are inspection tools, not stable
package metadata formats.

The IR retains static `Check` and send-adaptation metadata for inspection; these
are not runtime type checks or wrapper allocation. Runtime operations use lexical
binding/value IDs, explicit capture lists and payload projection paths, full
message sends with wait-first/no-reply modes, and erased receiver shapes. Source
spans support diagnostics without making provenance pointers executable identity.
The Erlang emitter omits proof metadata and empty receiver clauses.

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

Compilation units are packages containing file-derived modules. Local dependency
units are currently linked from source into one generated program; module SCCs
are checked in dependency order. See `docs/modules.md` for declarations, imports,
and entrypoint selection. Future serialized separate compilation is package-level,
not source-file-level. Packages will export type metadata and an ABI contract. Consumers can derive subsumption
plans without concrete implementation knowledge. No package-local selector IDs,
physical interface slots, or concrete-callee specialization may be required for
correctness. Package metadata formats and nonidentity adapter linkage are deferred.

## Linked Globals

Generated entry code initializes declarative globals once per session before
sending the configured initial message. A global lookup returns the stored value;
actors and aliases retain identity. Actors carry symbolic edges for globals their
behavior may reference, including dependencies of nested actor creation. The
session's global lookup table is metadata, not a GC root. Initialization retains
constructed values through the active entry actor until setup finishes.

`spawn_actor(Session, Fun, GlobalNames)` is a compiler-facing, same-session API:
its names refer only to globals in `Session`. It does not describe global lookups
against another session captured opaquely by arbitrary Erlang code. Embedders
must obtain and retain/export actual handles for those dependencies using the
existing host pinning contract. Likewise, host code must obtain and pin any
globals it intends to use before permitting their last live roots to disappear;
a global lookup does not resurrect a collected actor.

## Runtime API and lifecycle

The generated module exports `entry(Session)`. `aspen_runtime:start_session/0`
creates a session owned by its calling process; `spawn_actor/2` registers every
actor before returning its handle. Creation is serialized by the session so a
concurrent shutdown cannot miss a newly created actor. `shutdown/1` kills all
registered processes and waits for their termination. Owner death also shuts the
session down. These lifecycle monitors do not resolve outstanding Aspen calls.

`aspen_runtime:run(Module, Timeout)` waits for generated entry setup and the
configured initial no-reply send to complete, then traces
until remaining actor work has drained. It returns `ok`, `{error, timeout}`, or an
entry-failure error. The same deadline covers entry execution and draining;
`infinity` disables it. Only the idle, empty syscall singleton is exempt from
preventing completion. Autonomous active loops continue running after the entry
returns. Host-pinned actors can prevent draining, so embedders with their own
completion contract can instead call `shutdown/1` explicitly. Source-level host
output is available through `syscall`; a source-level shutdown primitive remains
deferred.

## Actor liveness collection

Actor garbage is defined by possible future execution, not merely reachability
from the entrypoint. Executing actors and runnable messages keep autonomous
cycles alive. A mutually capturing group of idle actors without pending work or
an external activation path is garbage. A suspended call is not inherently a
root: holders of its live reply capability provide the activation path instead.
Ordinary requests queued behind a suspended call cannot themselves resume it.

The compiler emits `receive_request(Session, Captures)` at actor receive-loop
boundaries and `call(Target, Message, LiveTerms)` at suspension points. Backward
liveness over the generated block identifies continuation terms; actor captures
remain live across calls because the next loop iteration needs them. Static
checking/adaptation evidence is not a runtime reference. Nested selector payloads
can contain actor and reply handles and must be traversed too.

Collection must preserve ownership during message handoffs: the sender cannot
become collectible before the recipient or pending delivery accounts for the
work and its references. Reply delegation follows the reply capability, not the
original callee's lifetime. Collection is not a new error reply or a change to
first-response semantics.

The initial implementation uses a per-session coordinator for graph transitions
and tracing rather than reference counts. Actors need not stop together or
respond to a global pause request. Coordinator operations can queue during a
trace; this is not a lock-free or bounded-latency collector. Opaque Erlang code,
native I/O, and exported host handles require conservative retention. Precision
and distribution of graph accounting are future optimizations, not assumptions
needed for safety.

## Message transport

The internal transport ABI is `{aspen_request, Message, ReplyTarget}`, where a
no-reply send uses `none`. Reply handles are `{aspen_reply, Alias}` and reply
transport is `{aspen_response, Alias, Value}` delivered to that alias. Managed
transport passes through the owning session coordinator so pending work remains
accounted for during delivery. Direct Erlang mailbox writes bypass this protocol
and are unsupported for collectable actors. `call/2` is the conservative embedding
API; generated code uses `call/3` with explicit continuation roots. Both use an
explicit-un-alias process alias and selective receive; cleanup first
deactivates the alias, then drains only responses carrying that exact alias.
No receiver monitor or request timer participates in this protocol. A failed
receiver emits the normal BEAM error diagnostic; its death does not resolve the
call, since a delegate could hold the reply endpoint. Liveness tracing may collect
an unreachable suspended caller only when no root reaches its activation graph.

## Global Syscall Actor

The global `syscall` is an ordinary, first-class actor with interface
`{ write: int data: bytes -> int }`. It is a fallback for unbound references named
`syscall` in every lexical scope; user bindings shadow it normally. The runtime
provides one shared syscall actor per session and registers it for session
shutdown. Aliasing or passing the global preserves that endpoint's identity.

`syscall write: 1 data: "Hello!\n".` invokes POSIX `write(2)` against descriptor
1 in the BEAM VM process. The method accepts strings by `string <: bytes` and
replies with the number of bytes written, or negative platform-native `errno`.
There is one native write attempt: short writes, `EINTR`, and other failures are
returned without retries. Descriptors outside the native integer range return
negative `EBADF` rather than being truncated. No newline, encoding conversion,
formatting, or flushing protocol is implicit. The result counts bytes, not
Unicode characters. A replying send waits for the attempt even when its result
is discarded, so entry completion does not overtake that write.

This is raw descriptor access, not Erlang's group-leader I/O abstraction and not
a capability sandbox. Descriptors refer to the VM's descriptor table, including
inherited descriptors; conventionally 1 is stdout and 2 is stderr. Applications
must not guess or interfere with descriptors owned by the VM. Native `errno`
numbers are platform-dependent and are not a portable tagged error vocabulary.

The bridge lives in `aspen_syscall.erl` and `aspen_syscall_nif.c`. The NIF runs on
an Erlang dirty I/O scheduler, not a normal scheduler. A blocking OS write may
still remain blocked independently of the actor or runner deadline; scheduling
it as dirty I/O does not make the syscall cancellable. As with any native NIF,
this code executes inside the VM rather than in an isolated helper process.

For a manually embedded runtime, build with
`make -C crates/aspenc/runtime` in the development shell. This requires `erl`,
`erlc`, `make`, a C compiler, and Erlang NIF headers. Deploy `aspen_runtime.beam`,
`aspen_syscall.beam`, and `aspen_syscall_nif.so` together on the Erlang code path;
the syscall module loads its shared library relative to its own BEAM file.

The supported toolchain baseline is OTP 28 (tested on 28.2). Generated source is
compiled by `erlc`, not translated directly to bytecode. Linked package units emit
one generated module plus the runtime modules and native syscall bridge;
serialized separate compilation, nonidentity adaptations, and supervision remain deferred.
