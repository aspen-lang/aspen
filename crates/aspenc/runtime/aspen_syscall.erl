%% Native syscall bridge. The shared library lives beside this module's BEAM file.
-module(aspen_syscall).
-on_load(init/0).
-export([available/0, write/2]).

init() ->
    Directory = filename:dirname(code:which(?MODULE)),
    erlang:load_nif(filename:join(Directory, "aspen_syscall_nif"), 0).

available() -> ok.
write(_Fd, _Bytes) -> erlang:nif_error(nif_not_loaded).
