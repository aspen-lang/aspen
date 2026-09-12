# BEAM runtime

Build with Erlang/OTP 28+, a C compiler, and POSIX headers:

```sh
nix develop
make -C crates/aspenc/runtime
```

Keep `aspen_runtime.beam`, `aspen_syscall.beam`, and
`aspen_syscall_nif.so` together, and add that directory to Erlang's code path
with `-pa`. The native library is resolved relative to `aspen_syscall.beam`,
not the current working directory. `CC`, `CFLAGS`, `ERL`, `ERLC`, and
`ERL_ROOT` may be overridden when building. The native bridge targets POSIX
systems (Linux and macOS), not Windows.

## Native write contract

`syscall write: fd data: bytes` uses `write(2)` in the BEAM process. Descriptors
are real process-wide OS descriptors: stdout is 1, stderr is 2, and inherited
open descriptors are accessible. This is an ambient capability, not a sandbox.
Runtime-internal descriptors must not be modified by Aspen programs.

Each request performs exactly one write attempt, replies with the byte count,
and preserves partial writes. Errors return negative platform `errno` numbers;
there are no EINTR retries or text conversions. Negative descriptors and values
above C `INT_MAX` return `-EBADF` without narrowing the integer. Buffers above
`SSIZE_MAX` return `-EINVAL` rather than relying on implementation-defined
behavior. An empty buffer is still passed to the OS, including descriptor
validation according to the destination's OS semantics.

The native function runs on a dirty I/O scheduler. A blocking write still blocks
that scheduler thread and the session's syscall actor; it cannot be cancelled
by an Aspen deadline or session shutdown while inside the OS. Consequently,
shutdown may wait for a blocked syscall to return. Embedders must manage
nonblocking descriptors or use an external process deadline where necessary.
The returned count means the OS accepted those bytes, not that data is durable
or that a terminal displayed it.

The compiled startup path obtains the session's syscall actor through the
internal `syscall/1` ABI and passes it to the entry actor. Repeated internal
requests return the same PID. Aspen code has no global acquisition mechanism;
it must receive or capture the capability. The session owns that actor's
lifetime like other actors. Missing native libraries fail during startup rather
than creating an actor that would silently leave the first call waiting forever.

## Actor collection and embedding

The runtime traces possible future activation within each session. Active work,
pending runnable requests/responses, and pinned host handles are roots. Idle
cycles without an activation path are reclaimed; unrooted autonomous message
loops remain live. A suspended caller is not automatically a root, and ordinary
requests queued behind its reply wait do not make it runnable.

Generated code publishes captures through `receive_request/2` and live
continuation terms through `call/3`. Executing opaque Erlang functions and
foreign calls are retained conservatively. The collector does not inspect BEAM
process heaps or stop all processes together. A session coordinator serializes
graph changes and message handoffs; calls into it can queue during tracing.

Embedding rules:

- Use `aspen_runtime:send/2` and `call/2` for managed actor communication. Direct
  Erlang mailbox writes bypass ownership tracking and are not a supported way to
  deliver Aspen requests or replies. Reply handles must originate from runtime
  calls; manually constructed raw Erlang aliases are not supported.
- `spawn_actor/2` invoked by a host returns a pinned actor. Actors created by
  managed code are instead traced from their actual activation paths.
- `pin(Session, Pid)` and `unpin(Session, Pid)` set a session-wide host retention
  flag, not a reference count. Only unpin after **all** external holders have
  released the handle and no external operation can use it again.
- Handles exported to foreign code or another session are conservatively pinned.
  There is no distributed cycle collection across sessions. This retention also
  applies to callers whose reply capabilities escape to foreign code; consuming
  the reply does not automatically undo an export pin. Foreign unmanaged reply
  delivery may conservatively retain duplicate payloads. Treat native/host
  boundaries as ownership boundaries, not as transparent tracing links.
- `collect(Session)` requests a synchronous trace and returns the number selected
  for reclamation. Process termination follows asynchronously; use monitors if
  termination must be observed. `gc_stats(Session)` exposes graph counts for
  diagnostics. Automatic collection also runs periodically.

The syscall singleton remains a session root. A blocking native write is active
work, never garbage merely because its caller drops references. Collection does
not cancel I/O or synthesize failure replies. After entry completion, the runner
waits for the remaining graph to drain before shutdown, excluding only the idle
syscall singleton. Its deadline covers both entry execution and draining.
