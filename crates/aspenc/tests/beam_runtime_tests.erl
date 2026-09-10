%% Real transport tests: completion is explicit and every wait has a deadline.
-module(beam_runtime_tests).
-export([run/0, trace_entry/1]).

run() ->
    aliases(),
    delegation(),
    sessions(),
    concurrent_replies(),
    owner_death(),
    mailbox_order(),
    ok.

aliases() ->
    Caller = self(),
    Server = spawn(fun() ->
        receive
            {aspen_request, first, Reply1} ->
                %% Force duplicates to be queued before the first is accepted.
                true = erlang:suspend_process(Caller),
                aspen_runtime:send(Reply1, first),
                [aspen_runtime:send(Reply1, duplicate) || _ <- lists:seq(1, 100)],
                Caller ! {aspen_request, ordinary, none},
                true = erlang:resume_process(Caller),
                receive
                    {aspen_request, second, Reply2} ->
                        false = (Reply1 =:= Reply2),
                        aspen_runtime:send(Reply1, late),
                        aspen_runtime:send(Reply2, second),
                        Caller ! aliases_done
                after 2000 -> error(second_request_timeout)
                end
        after 2000 -> error(first_request_timeout)
        end
    end),
    first = aspen_runtime:call(Server, first),
    second = aspen_runtime:call(Server, second),
    receive aliases_done -> ok after 2000 -> error(aliases_timeout) end,
    receive {aspen_request, ordinary, none} -> ok
    after 0 -> error(ordinary_request_consumed_by_wait) end,
    receive {aspen_response, _, Unexpected} -> error({stale_reply, Unexpected})
    after 0 -> ok end.

delegation() ->
    Caller = self(),
    Delegate = spawn(fun() ->
        receive {reply_later, Reply, Server} ->
            Monitor = monitor(process, Server),
            receive {'DOWN', Monitor, process, Server, _} -> ok
            after 2000 -> error(server_exit_timeout) end,
            aspen_runtime:send(Reply, delegated),
            Caller ! delegate_done
        after 2000 -> error(delegation_timeout) end
    end),
    Server = spawn(fun() ->
        receive {aspen_request, go, Reply} ->
            Delegate ! {reply_later, Reply, self()}
        after 2000 -> error(delegation_request_timeout) end
    end),
    delegated = aspen_runtime:call(Server, go),
    receive delegate_done -> ok after 2000 -> error(delegate_done_timeout) end.

sessions() ->
    Session = aspen_runtime:start_session(),
    A = aspen_runtime:spawn_actor(Session, fun wait/0),
    B = aspen_runtime:spawn_actor(Session, fun wait/0),
    false = (A =:= B),
    true = is_process_alive(A),
    true = is_process_alive(B),
    MA = monitor(process, A),
    MB = monitor(process, B),
    ok = aspen_runtime:shutdown(Session),
    receive {'DOWN', MA, process, A, _} -> ok after 2000 -> error(shutdown_a) end,
    receive {'DOWN', MB, process, B, _} -> ok after 2000 -> error(shutdown_b) end.

wait() -> receive stop -> ok end.

trace_entry(Module) ->
    Session = aspen_runtime:start_session(),
    Parent = self(),
    Entry = spawn(fun() ->
        receive start -> Module:entry(Session), Parent ! entry_complete end
    end),
    1 = erlang:trace(Entry, true, [send, {tracer, self()}]),
    Entry ! start,
    Messages = collect_requests(Entry, []),
    [{aspen_atomic, make}, {aspen_atomic, one}, {aspen_atomic, two},
     {aspen_keyword, {left, right}, {{aspen_atomic, first}, {aspen_atomic, second}}}]
        = Messages,
    ok = aspen_runtime:shutdown(Session).

collect_requests(Entry, Acc) ->
    receive
        {trace, Entry, send, {aspen_request, Message, _Reply}, _Destination} ->
            collect_requests(Entry, [Message | Acc]);
        {trace, Entry, send, _, _} -> collect_requests(Entry, Acc);
        entry_complete ->
            Delivered = erlang:trace_delivered(Entry),
            collect_delivered(Entry, Delivered, Acc)
    after 3000 -> error(entry_completion_timeout)
    end.

concurrent_replies() ->
    [concurrent_reply() || _ <- lists:seq(1, 100)],
    ok.

concurrent_reply() ->
    Caller = self(),
    Server = spawn(fun() ->
        receive {aspen_request, race, Reply} ->
            Coordinator = self(),
            [spawn(fun() ->
                [aspen_runtime:send(Reply, winner) || _ <- lists:seq(1, 10)],
                Coordinator ! sender_done
            end) || _ <- lists:seq(1, 8)],
            [receive sender_done -> ok after 2000 -> error(sender_timeout) end
                || _ <- lists:seq(1, 8)],
            Caller ! race_done
        after 2000 -> error(race_request_timeout) end
    end),
    winner = aspen_runtime:call(Server, race),
    receive race_done -> ok after 2000 -> error(race_timeout) end,
    receive {aspen_response, _, _} -> error(race_left_duplicate)
    after 0 -> ok end.

owner_death() ->
    Parent = self(),
    Owner = spawn(fun() ->
        Session = aspen_runtime:start_session(),
        Actor = aspen_runtime:spawn_actor(Session, fun wait/0),
        Parent ! {owned, Session, Actor},
        receive finish -> ok end
    end),
    receive {owned, Session, Actor} ->
        MS = monitor(process, Session),
        MA = monitor(process, Actor),
        Owner ! finish,
        receive {'DOWN', MS, process, Session, _} -> ok
        after 2000 -> error(owner_session_leak) end,
        receive {'DOWN', MA, process, Actor, _} -> ok
        after 2000 -> error(owner_actor_leak) end
    after 2000 -> error(owner_setup_timeout) end.

mailbox_order() ->
    Parent = self(),
    Session = aspen_runtime:start_session(),
    Actor = aspen_runtime:spawn_actor(Session, fun() -> ordered_loop(Parent) end),
    aspen_runtime:send(Actor, first),
    aspen_runtime:send(Actor, second),
    acknowledged = aspen_runtime:call(Actor, barrier),
    receive {order, Value1} -> first = Value1 after 2000 -> error(first_timeout) end,
    receive {order, Value2} -> second = Value2 after 2000 -> error(second_timeout) end,
    ok = aspen_runtime:shutdown(Session).

ordered_loop(Parent) ->
    receive
        {aspen_request, barrier, Reply} ->
            aspen_runtime:send(Reply, acknowledged), ordered_loop(Parent);
        {aspen_request, Message, _} ->
            Parent ! {order, Message}, ordered_loop(Parent)
    end.

collect_delivered(Entry, Delivered, Acc) ->
    receive
        {trace, Entry, send, {aspen_request, Message, _}, _} ->
            collect_delivered(Entry, Delivered, [Message | Acc]);
        {trace, Entry, send, _, _} -> collect_delivered(Entry, Delivered, Acc);
        {trace_delivered, Entry, Delivered} -> lists:reverse(Acc)
    after 3000 -> error(trace_delivery_timeout)
    end.
