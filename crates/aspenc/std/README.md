# Aspen Standard Library

The compiler embeds this package's source so installed binaries do not require
an Aspen checkout. `std` is reserved and available without a manifest dependency;
imports remain explicit.

## Runtime Capability

```aspen
import std/runtime (type Syscall).

export let main = {
  def start: Syscall syscall =>
    syscall write: 1 data: "Hello, world!\n".
}.
```

Use `entry: {actor: app/main, message: start}` in a package named `app` when this
actor is in `src/index.aspen`. Startup sends `start:` with the native capability
as its payload. An entrypoint must accept this message without replying.

`Syscall` is a transparent structural interface, not a value or a constructor.
The actor implementing it belongs to the runtime. Other actors must receive or
capture its reference explicitly; importing the type grants no I/O authority.

`write: int data: bytes -> int` performs one POSIX write against a descriptor in
the runtime process. It returns the byte count or negative native `errno`, with
no retry or automatic newline. This is broad descriptor authority, not a portable
platform abstraction or a sandbox. Use a wrapper actor to enforce policies such
as stdout-only output. Narrowing a reference's structural type restricts the
operations available to checked code but does not change the underlying actor.

The interface and native implementation ship together. Changes to this contract
must update runtime conformance tests as well as its declaration.
