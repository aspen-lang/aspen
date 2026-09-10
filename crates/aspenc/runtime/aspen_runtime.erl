%% Aspen's internal ABI. Requires Erlang/OTP 28 or newer.
-module(aspen_runtime).
-export([send/2, call/2, project/2, start_session/0, spawn_actor/2, shutdown/1, run/2]).

project({aspen_keyword, _, Payloads}, Index) -> element(Index + 1, Payloads);
project({Tag, Payload}, 0) when Tag =:= aspen_plus_operator;
                                Tag =:= aspen_minus_operator;
                                Tag =:= aspen_star_operator;
                                Tag =:= aspen_slash_operator -> Payload.

send(Pid, Message) when is_pid(Pid) ->
    Pid ! {aspen_request, Message, none},
    ok;
send({aspen_reply, Alias}, Message) when is_reference(Alias) ->
    Alias ! {aspen_response, Alias, Message},
    ok.

call(Pid, Message) when is_pid(Pid) ->
    Alias = erlang:alias([explicit_unalias]),
    try
        Pid ! {aspen_request, Message, {aspen_reply, Alias}},
        receive
            {aspen_response, Alias, Value} -> Value
        end
    after
        %% Deactivation drops future deliveries, not duplicates already queued.
        erlang:unalias(Alias),
        flush_replies(Alias)
    end.

flush_replies(Alias) ->
    receive
        {aspen_response, Alias, _} -> flush_replies(Alias)
    after 0 -> ok
    end.

%% The session owns creation so shutdown cannot race an unregistered child.
%% Monitoring is lifecycle bookkeeping only: actor death never resolves a call.
start_session() ->
    Owner = self(),
    spawn(fun() ->
        OwnerMonitor = monitor(process, Owner),
        session(OwnerMonitor, #{})
    end).

spawn_actor(Session, Fun) when is_function(Fun, 0) ->
    Ref = monitor(process, Session),
    Session ! {spawn_actor, self(), Ref, Fun},
    receive
        {Ref, Pid} ->
            demonitor(Ref, [flush]),
            Pid;
        {'DOWN', Ref, process, Session, Reason} ->
            error({aspen_session_closed, Reason})
    end.

shutdown(Session) ->
    Ref = monitor(process, Session),
    Session ! shutdown,
    receive {'DOWN', Ref, process, Session, _} -> ok end.

session(OwnerMonitor, Actors) ->
    receive
        {spawn_actor, From, Ref, Fun} ->
            {Pid, ActorMonitor} = spawn_monitor(Fun),
            From ! {Ref, Pid},
            session(OwnerMonitor, Actors#{ActorMonitor => Pid});
        {'DOWN', OwnerMonitor, process, _, _} ->
            stop_actors(Actors);
        {'DOWN', Ref, process, _, _} ->
            session(OwnerMonitor, maps:remove(Ref, Actors));
        shutdown ->
            stop_actors(Actors)
    end.

stop_actors(Actors) ->
    maps:foreach(fun(_, Pid) -> exit(Pid, kill) end, Actors),
    await_stopped(Actors).

await_stopped(Actors) when map_size(Actors) =:= 0 -> ok;
await_stopped(Actors) ->
    receive
        {'DOWN', Ref, process, _, _} ->
            await_stopped(maps:remove(Ref, Actors))
    end.

%% This runner explicitly chooses entry completion as its shutdown boundary.
%% Embedders can instead retain a session until their own completion signal.
run(Module, Timeout) when Timeout =:= infinity; is_integer(Timeout), Timeout >= 0 ->
    Session = start_session(),
    Gate = make_ref(),
    Entry = spawn_actor(Session, fun() ->
        receive {start, Gate} -> Module:entry(Session) end
    end),
    Ref = monitor(process, Entry),
    Entry ! {start, Gate},
    Result = receive
        {'DOWN', Ref, process, Entry, normal} -> ok;
        {'DOWN', Ref, process, Entry, Reason} -> {error, {entry_failed, Reason}}
    after Timeout ->
        {error, timeout}
    end,
    shutdown(Session),
    demonitor(Ref, [flush]),
    Result.
