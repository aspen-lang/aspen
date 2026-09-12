%% Aspen's internal ABI. Requires Erlang/OTP 28 or newer.
-module(aspen_runtime).
-export([send/2, call/2, call/3, project/2, start_session/0, spawn_actor/2, spawn_actor/3,
         initialize_globals/2, global/2, define_global/3,
         syscall/1, receive_request/2, collect/1, pin/2, unpin/2, gc_stats/1,
         is_string/1, shutdown/1, run/2]).

is_string(Value) when is_binary(Value) ->
    try unicode:characters_to_binary(Value, utf8, utf8) of
        Result when is_binary(Result) -> true;
        _ -> false
    catch error:_ -> false end;
is_string(_) -> false.

project({aspen_keyword, _, Payloads}, Index) -> element(Index + 1, Payloads);
project({Tag, Payload}, 0) when Tag =:= aspen_plus_operator;
                                Tag =:= aspen_minus_operator;
                                Tag =:= aspen_star_operator;
                                Tag =:= aspen_slash_operator -> Payload.

rpc(Session, Request) ->
    Ref = monitor(process, Session),
    Session ! {gc_rpc, self(), Ref, Request},
    receive
        {Ref, Result} -> demonitor(Ref, [flush]), Result;
        {'DOWN', Ref, process, Session, Reason} -> error({aspen_session_closed, Reason})
    end.

owner(Pid) when is_pid(Pid) -> registry_owner(Pid);
owner({aspen_reply, Alias}) -> registry_owner(Alias);
owner(_) -> undefined.

send({aspen_reply, Alias} = Target, Message) ->
    case owner(Target) of
        undefined -> ok;
        {host, _} -> export_terms(Message), direct_send(Target, Message, none);
        Session -> export_cross_session(Session, [Target, Message]),
                   rpc(Session, {send, {aspen_reply, Alias}, Message})
    end;
send(Target, Message) ->
    case owner(Target) of
        undefined ->
            export_terms([Target, Message]),
            direct_send(Target, Message, none);
        Session ->
            export_cross_session(Session, [Target, Message]),
            rpc(Session, {send, Target, Message})
    end.

direct_send(Pid, Message, Reply) when is_pid(Pid) ->
    Pid ! {aspen_request, Message, Reply}, ok;
direct_send({aspen_reply, Alias}, Message, _) when is_reference(Alias) ->
    Alias ! {aspen_response, Alias, Message}, ok.

call(Pid, Message) -> call(Pid, Message, conservative).
call(Pid, Message, Live) when is_pid(Pid) ->
    ensure_registry(),
    Alias = erlang:alias([explicit_unalias]),
    Session = owner(Pid),
    case Session of
        undefined ->
            ets:insert(aspen_actor_registry, {Alias, {host, self()}}),
            ets:info(aspen_actor_registry, owner) ! {watch_host_alias, self(), Alias};
        _ -> ok
    end,
    try
        case Session of
            undefined ->
                %% Foreign code may retain any exported handle indefinitely.
                export_terms([Pid, Message]),
                direct_send(Pid, Message, {aspen_reply, Alias});
            _ ->
                export_cross_session(Session, [Pid, Message]),
                rpc(Session, {call, Pid, Message, Alias, Live})
        end,
        receive
            {aspen_response, Alias, Value} ->
                case Session of
                    undefined -> import_terms(Value);
                    _ -> rpc(Session, {answered, Alias, Value})
                end,
                Value
        end
    after
        erlang:unalias(Alias),
        case Session of
            undefined -> registry_remove(Alias),
                ets:info(aspen_actor_registry, owner) ! {forget_host_alias, Alias};
            _ -> rpc(Session, {forget_alias, Alias})
        end,
        flush_replies(Alias)
    end.

flush_replies(Alias) ->
    receive {aspen_response, Alias, _} -> flush_replies(Alias)
    after 0 -> ok end.

receive_request(Session, Captures) ->
    rpc(Session, {idle, Captures}),
    receive
        {aspen_request, Message, Reply} = Request ->
            rpc(Session, {activate, Message, Reply}),
            Request
    end.

spawn_actor(Session, Fun) when is_function(Fun, 0) ->
    export_cross_session(Session, Fun),
    rpc(Session, {spawn, Fun}).
%% Compiler-facing: names refer only to globals in this same Session.
%% Names are routing metadata, never roots. Each actor carries only the global
%% edges its code can use, including dependencies of nested actor creation.
spawn_actor(Session, Fun, Globals) when is_function(Fun, 0) ->
    export_cross_session(Session, Fun),
    rpc(Session, {spawn, Fun, Globals}).
initialize_globals(Session, Fun) ->
    case rpc(Session, begin_globals) of
        initialize -> Fun(), rpc(Session, end_globals);
        ready -> ok;
        busy -> error(aspen_globals_initializing)
    end.
define_global(Session, Name, Value) -> rpc(Session, {define_global, Name, Value}).
global(Session, Name) -> rpc(Session, {global, Name}).

syscall(Session) ->
    ok = aspen_syscall:available(),
    rpc(Session, syscall).

syscall_loop(Session) ->
    case receive_request(Session, []) of
        {aspen_request, {aspen_keyword, {write, data}, {Fd, Bytes}}, Reply}
          when is_integer(Fd), Fd >= -9223372036854775808,
               Fd =< 9223372036854775807, is_binary(Bytes) ->
            Result = aspen_syscall:write(Fd, Bytes),
            case Reply of none -> ok; _ -> send(Reply, Result) end,
            syscall_loop(Session);
        {aspen_request, Message, _} -> error({aspen_unmatched, Message})
    end.

collect(Session) -> rpc(Session, collect).
pin(Session, Pid) -> rpc(Session, {pin, Pid, true}).
unpin(Session, Pid) -> rpc(Session, {pin, Pid, false}).
gc_stats(Session) -> rpc(Session, stats).
shutdown(Session) ->
    Ref = monitor(process, Session), Session ! shutdown,
    receive {'DOWN', Ref, process, Session, _} -> ok end.

start_session() ->
    ensure_registry(),
    Owner = self(),
    spawn(fun() ->
        Monitor = monitor(process, Owner),
        ets:info(aspen_actor_registry, owner) ! {watch, self()},
        erlang:send_after(100, self(), gc_tick),
        try session(#{owner => Monitor, actors => #{}, aliases => #{}, syscall => undefined, globals => #{}, globals_status => new})
        after cleanup_registry() end
    end).

%% The coordinator linearizes transitions, not execution: no actor is suspended
%% for tracing. Transfer ownership is recorded before delivery or acknowledgement.
session(State) ->
    Owner = maps:get(owner, State),
    receive
        {gc_rpc, From, Ref, Request} ->
            {Result, Next} = request(From, Request, State),
            From ! {Ref, Result}, session(Next);
        {gc_export, Terms} -> session(pin_terms(Terms, State));
        {'DOWN', Owner, process, _, _} -> stop_actors(State);
        {'DOWN', _, process, Pid, _} ->
            registry_remove(Pid),
            Next = State#{actors := maps:remove(Pid, maps:get(actors, State)),
                syscall := case maps:get(syscall, State) of Pid -> undefined; Other -> Other end},
            session(remove_owner_aliases(Pid, Next));
        gc_tick ->
            {_, Next} = sweep(State),
            erlang:send_after(100, self(), gc_tick), session(Next);
        shutdown -> stop_actors(State)
    end.

request(_, begin_globals, #{globals_status := new} = State) ->
    {initialize, State#{globals_status := initializing}};
request(_, begin_globals, #{globals_status := ready} = State) -> {ready, State};
request(_, begin_globals, State) -> {busy, State};
request(_, end_globals, State) -> {ok, State#{globals_status := ready}};
request(From, {define_global, Name, Value}, State) ->
    Globals = maps:get(globals, State),
    {ok, add_refs(From, Value, State#{globals := Globals#{Name => Value}})};
request(From, {global, Name}, State) ->
    Value = maps:get(Name, maps:get(globals, State)),
    Next = case managed(From, State) of
        true -> add_refs(From, Value, State);
        false -> pin_terms(Value, State)
    end,
    {Value, Next};
request(From, {spawn, Fun, Globals}, State) ->
    {Pid, Next} = new_actor(Fun, not managed(From, State), State),
    Final = update(Pid, fun(A) -> A#{globals => [{aspen_global, N} || N <- Globals]} end, Next),
    {Pid, add_refs(From, [Pid], Final)};
request(From, {spawn, Fun}, State) ->
    {Pid, Next} = new_actor(Fun, not managed(From, State), State),
    {Pid, add_refs(From, [Pid], Next)};
request(From, syscall, #{syscall := undefined} = State) ->
    Session = self(),
    {Pid, Next} = new_actor(fun() -> syscall_loop(Session) end, true, State),
    {Pid, add_refs(From, [Pid], Next#{syscall := Pid})};
request(From, syscall, #{syscall := Pid} = State) ->
    {Pid, add_refs(From, [Pid], State)};
request(From, {idle, Captures}, State) ->
    {ok, update(From, fun(A) -> A#{status := idle, refs := refs(Captures)} end, State)};
request(From, {activate, Message, Reply}, State) ->
    {ok, update(From, fun(A) ->
        Pending = case maps:get(pending, A) of [] -> []; [_|Rest] -> Rest end,
        A#{status := active, refs := union(maps:get(refs, A), refs([Message, Reply])), pending := Pending}
    end, State)};
request(From, {send, Target, Message}, State) ->
    {ok, deliver(From, Target, Message, none, State)};
request(From, {call, Target, Message, Alias, Live}, State) ->
    register(Alias),
    Aliases = maps:get(aliases, State),
    S1 = State#{aliases := Aliases#{Alias => From}},
    S2 = update(From, fun(A) ->
        R = case Live of conservative -> maps:get(refs, A); _ -> refs(Live) end,
        A#{status := waiting, refs := R, response => []}
    end, S1),
    {ok, deliver(From, Target, Message, {aspen_reply, Alias}, S2)};
request(From, {answered, Alias, Value}, State) ->
    S1 = update(From, fun(A) ->
        A#{status := active, refs := union(maps:get(refs, A), refs(Value)), response => []}
    end, State),
    S2 = case managed(From, State) of true -> S1; false -> pin_terms(Value, S1) end,
    {ok, forget_alias(Alias, S2)};
request(_, {forget_alias, Alias}, State) -> {ok, forget_alias(Alias, State)};
request(_, {pin, Pid, Pin}, State) ->
    {ok, update(Pid, fun(A) -> A#{pinned := Pin} end, State)};
request(_, {export, Terms}, State) -> {ok, pin_terms(Terms, State)};
request(From, {import, Terms}, State) -> {ok, add_refs(From, Terms, State)};
request(_, drained, State) ->
    {_, Next} = sweep(State),
    Syscall = maps:get(syscall, Next),
    Drained = maps:fold(fun(Pid, A, Acc) ->
        Acc andalso (maps:get(status, A) =:= retiring orelse
            (Pid =:= Syscall andalso maps:get(status, A) =:= idle andalso
             maps:get(pending, A) =:= []))
    end, true, maps:get(actors, Next)),
    {Drained, Next};
request(_, collect, State) -> sweep(State);
request(_, stats, State) ->
    Actors = maps:values(maps:get(actors, State)),
    Counts = [{K, length([A || A <- Actors, maps:get(status, A) =:= K])}
              || K <- [active, idle, waiting]],
    {maps:from_list([{actors, length(Actors)},
        {pinned, length([A || A <- Actors, maps:get(pinned, A)])} | Counts]), State}.

new_actor(Fun, Pin, State) ->
    Session = self(), Gate = make_ref(),
    {Pid, Monitor} = spawn_monitor(fun() ->
        put(aspen_session, Session),
        receive {start, Gate} -> Fun() end
    end),
    register(Pid),
    Actors = maps:get(actors, State),
    A = #{monitor => Monitor, status => active, refs => refs(Fun),
          pending => [], response => [], pinned => Pin},
    Next = State#{actors := Actors#{Pid => A}},
    Pid ! {start, Gate},
    {Pid, Next}.

managed(Pid, State) -> maps:is_key(Pid, maps:get(actors, State)).
update(Pid, F, State) ->
    Actors = maps:get(actors, State),
    case maps:find(Pid, Actors) of
        {ok, A} -> State#{actors := Actors#{Pid := F(A)}};
        error -> State
    end.
add_refs(Pid, Terms, State) ->
    update(Pid, fun(A) -> A#{refs := union(maps:get(refs, A), refs(Terms))} end, State).

%% Ordinary queued requests cannot resume a caller's selective reply receive.
%% A pending response can; it is retained until the caller acknowledges receipt.
deliver(From, Pid, Message, Reply, State) when is_pid(Pid) ->
    R = refs([Message, Reply]),
    Next = update(Pid, fun(A) ->
        A#{pending := maps:get(pending, A) ++ [R]}
    end, State),
    Final = case managed(Pid, State) of true -> Next; false -> pin_terms([Message, Reply], Next) end,
    direct_send(Pid, Message, Reply),
    add_refs(From, [Pid], Final);
deliver(_, {aspen_reply, Alias} = Target, Message, _, State) ->
    case maps:find(Alias, maps:get(aliases, State)) of
        {ok, {answered, _}} -> State;
        {ok, Caller} ->
            Aliases = maps:get(aliases, State),
            First = State#{aliases := Aliases#{Alias := {answered, Caller}}},
            Next = update(Caller, fun(A) ->
                A#{response := union(maps:get(response, A), refs(Message)), status := responding}
            end, First),
            Final = case managed(Caller, State) of true -> Next; false -> pin_terms(Message, Next) end,
            direct_send(Target, Message, none), Final;
        error -> State
    end.

forget_alias(Alias, State) ->
    registry_remove(Alias), State#{aliases := maps:remove(Alias, maps:get(aliases, State))}.
remove_owner_aliases(Pid, State) ->
    lists:foldl(fun(Alias, S) -> forget_alias(Alias, S) end, State,
        [Alias || {Alias, Owner} <- maps:to_list(maps:get(aliases, State)),
            Owner =:= Pid orelse Owner =:= {answered, Pid}]).

refs(Term) -> lists:usort(refs(Term, [])).
refs(Pid, Acc) when is_pid(Pid) -> [Pid | Acc];
refs({aspen_reply, Alias}, Acc) when is_reference(Alias) -> [{alias, Alias} | Acc];
refs({aspen_global, Name}, Acc) -> [{aspen_global, Name} | Acc];
refs({alias, Alias}, Acc) when is_reference(Alias) -> [{alias, Alias} | Acc];
refs(Tuple, Acc) when is_tuple(Tuple) -> refs(tuple_to_list(Tuple), Acc);
refs([H|T], Acc) -> refs(T, refs(H, Acc));
refs(Map, Acc) when is_map(Map) -> refs(maps:to_list(Map), Acc);
refs(Fun, Acc) when is_function(Fun) ->
    {env, Env} = erlang:fun_info(Fun, env), refs(Env, Acc);
refs(_, Acc) -> Acc.
union(A, B) -> lists:usort(A ++ B).

pin_terms(Terms, State) ->
    lists:foldl(fun(Pid, S) ->
        update(Pid, fun(A) -> A#{pinned := true} end, S)
    end, State, resolve_refs(refs(Terms), State)).
resolve_refs(Refs, State) ->
    lists:flatmap(fun
        ({aspen_global, Name}) -> refs(maps:get(Name, maps:get(globals, State), undefined));
        ({alias, Alias}) -> case maps:find(Alias, maps:get(aliases, State)) of
            {ok, {answered, _}} -> [];
            {ok, Pid} -> [Pid]; error -> [] end;
        (Pid) -> [Pid]
    end, Refs).

sweep(State) ->
    Actors = maps:filter(fun(_, A) -> maps:get(status, A) =/= retiring end,
                         maps:get(actors, State)),
    Roots = [Pid || {Pid, A} <- maps:to_list(Actors),
        maps:get(pinned, A) orelse maps:get(status, A) =:= active orelse
        maps:get(status, A) =:= responding orelse
        (maps:get(status, A) =:= idle andalso maps:get(pending, A) =/= [])],
    Live = mark(Roots, #{}, State),
    Dead = [Pid || Pid <- maps:keys(Actors), not maps:is_key(Pid, Live)],
    lists:foreach(fun(Pid) -> exit(Pid, kill), registry_remove(Pid) end, Dead),
    Next = lists:foldl(fun(Pid, S) -> update(Pid, fun(A) ->
        A#{status := retiring, pinned := false, refs := [], pending := [], response := []}
    end, S) end, State, Dead),
    {length(Dead), lists:foldl(fun remove_owner_aliases/2, Next, Dead)}.
mark([], Seen, _) -> Seen;
mark([Pid|Rest], Seen, State) ->
    case {maps:is_key(Pid, Seen), maps:find(Pid, maps:get(actors, State))} of
        {false, {ok, A}} ->
            Refs = maps:get(globals, A, []) ++ maps:get(refs, A) ++ lists:append(maps:get(pending, A)) ++ maps:get(response, A),
            mark(resolve_refs(Refs, State) ++ Rest, Seen#{Pid => true}, State);
        _ -> mark(Rest, Seen, State)
    end.

%% Routing metadata is not a liveness root. An independent table owner cleans
%% registrations even when a session coordinator exits unexpectedly.
registry_owner(Key) ->
    case ets:whereis(aspen_actor_registry) of
        undefined -> undefined;
        _ -> case ets:lookup(aspen_actor_registry, Key) of
            [{Key, Session}] -> Session; [] -> undefined end
    end.
ensure_registry() ->
    case ets:whereis(aspen_actor_registry) of
        undefined ->
            Parent = self(),
            spawn(fun() ->
                try ets:new(aspen_actor_registry, [named_table, public, set,
                        {read_concurrency, true}]) of
                    _ -> Parent ! registry_ready, registry_loop()
                catch error:badarg -> Parent ! registry_ready end
            end),
            receive registry_ready -> ok end;
        _ -> ok
    end.
registry_loop() ->
    receive
        {watch_host_alias, Host, Alias} ->
            Monitor = monitor(process, Host),
            put({host_alias, Monitor}, Alias),
            registry_loop();
        {forget_host_alias, Alias} ->
            lists:foreach(fun
                ({{host_alias, Monitor}, A}) when A =:= Alias ->
                    demonitor(Monitor, [flush]), erase({host_alias, Monitor});
                (_) -> ok
            end, get()),
            registry_loop();
        {watch, Session} -> monitor(process, Session), registry_loop();
        {'DOWN', Monitor, process, Session, _} ->
            case erase({host_alias, Monitor}) of
                undefined -> ets:match_delete(aspen_actor_registry, {'_', Session});
                Alias -> ets:delete(aspen_actor_registry, Alias)
            end,
            registry_loop()
    end.
register(Key) -> ets:insert(aspen_actor_registry, {Key, self()}), ok.
registry_remove(Key) -> ets:delete(aspen_actor_registry, Key), ok.
cleanup_registry() -> ets:match_delete(aspen_actor_registry, {'_', self()}), ok.

export_terms(Terms) ->
    %% A host/foreign runtime has no release protocol: exports remain pinned.
    Sessions = lists:usort([S || Ref <- refs(Terms),
        S <- [case Ref of {alias, A} -> owner({aspen_reply, A}); _ -> owner(Ref) end],
        is_pid(S)]),
    lists:foreach(fun(S) -> rpc(S, {export, Terms}) end, Sessions), ok.
export_cross_session(Session, Terms) ->
    %% Foreign handles are conservatively exported even when the sender and
    %% destination belong to the same session. Coordinators never call each other.
    Foreign = [R || R <- refs(Terms),
        owner(case R of {alias, A} -> {aspen_reply, A}; _ -> R end) =/= Session],
    export_terms(Foreign),
    case get(aspen_session) of Session -> ok; _ -> export_terms(Terms) end.
import_terms(Value) ->
    case get(aspen_session) of
        undefined -> export_terms(Value);
        Session -> rpc(Session, {import, Value})
    end.

stop_actors(State) ->
    Actors = maps:get(actors, State),
    maps:foreach(fun(Pid, _) -> exit(Pid, kill) end, Actors),
    maps:foreach(fun(Pid, A) ->
        Monitor = maps:get(monitor, A),
        receive {'DOWN', Monitor, process, Pid, _} -> ok end
    end, Actors).

%% Entry completion releases its roots; autonomous work must drain before exit.
run(Module, Timeout) when Timeout =:= infinity; is_integer(Timeout), Timeout >= 0 ->
    Deadline = case Timeout of infinity -> infinity;
        _ -> erlang:monotonic_time(millisecond) + Timeout end,
    Session = start_session(),
    Gate = make_ref(),
    Entry = spawn_actor(Session, fun() ->
        receive {start_entry, Gate} -> Module:entry(Session) end
    end),
    Ref = monitor(process, Entry),
    Entry ! {start_entry, Gate},
    Result = receive
        {'DOWN', Ref, process, Entry, normal} -> await_drained(Session, Deadline);
        {'DOWN', Ref, process, Entry, Reason} -> {error, {entry_failed, Reason}}
    after Timeout -> {error, timeout} end,
    shutdown(Session), demonitor(Ref, [flush]), Result.

await_drained(Session, Deadline) ->
    case Deadline =/= infinity andalso erlang:monotonic_time(millisecond) >= Deadline of
        true -> {error, timeout};
        false ->
            case rpc(Session, drained) of
                true -> ok;
                false -> receive after 5 -> await_drained(Session, Deadline) end
            end
    end.
