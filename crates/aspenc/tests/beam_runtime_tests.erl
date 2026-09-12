%% Real transport tests: completion is explicit and every wait has a deadline.
-module(beam_runtime_tests).
-export([run/0, trace_entry/1, entry/1]).

run() ->
    aliases(),
    delegation(),
    sessions(),
    concurrent_replies(),
    owner_death(),
    mailbox_order(),
    syscalls(),
    garbage_collection(),
    {error, timeout} = aspen_runtime:run(?MODULE, 50),
    ok.

%% The entry returns, but its autonomous child must keep run alive until timeout.
entry(Session) ->
    Actor = aspen_runtime:spawn_actor(Session, fun() -> runner_loop(Session) end),
    aspen_runtime:send(Actor, again).

runner_loop(Session) ->
    {aspen_request, again, none} = aspen_runtime:receive_request(Session, []),
    aspen_runtime:send(self(), again),
    runner_loop(Session).

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

syscalls() ->
    Session = aspen_runtime:start_session(),
    Syscall = aspen_runtime:syscall(Session),
    Syscall = aspen_runtime:syscall(Session),
    true = is_pid(Syscall),
    0 = aspen_runtime:call(Syscall, {aspen_keyword, {write, data}, {1, <<>>}}),
    BadFd = aspen_runtime:call(Syscall, {aspen_keyword, {write, data}, {-1, <<>>}}),
    true = BadFd < 0,
    BadFd = aspen_runtime:call(Syscall,
        {aspen_keyword, {write, data}, {9223372036854775807, <<>>}}),
    3 = aspen_runtime:call(Syscall, {aspen_keyword, {write, data}, {1, <<65, 0, 255>>}}),
    4 = aspen_runtime:call(Syscall,
        {aspen_keyword, {write, data}, {1, <<240, 159, 152, 128>>}}),
    true = aspen_runtime:is_string(<<>>),
    true = aspen_runtime:is_string(<<240, 159, 152, 128>>),
    false = aspen_runtime:is_string(<<255>>),
    false = aspen_runtime:is_string(<<195>>),
    false = aspen_runtime:is_string(<<237, 160, 128>>),
    false = aspen_runtime:is_string(123),
    Monitor = monitor(process, Syscall),
    ok = aspen_runtime:shutdown(Session),
    receive {'DOWN', Monitor, process, Syscall, _} -> ok
    after 2000 -> error(syscall_leaked) end.

wait() -> receive stop -> ok end.

trace_entry(Module) ->
    Session = aspen_runtime:start_session(),
    Parent = self(),
    Entry = spawn(fun() ->
        receive start -> Module:entry(Session), Parent ! entry_complete end
    end),
    %% Observe the language send boundary, not the runtime's delivery process.
    %% Collection may route transport through the session coordinator.
    _ = erlang:trace_pattern({aspen_runtime, call, 3}, true, [local]),
    _ = erlang:trace_pattern({aspen_runtime, send, 2}, true, [local]),
    1 = erlang:trace(Entry, true, [call, {tracer, self()}]),
    Entry ! start,
    Messages = collect_requests(Entry, []),
    [{aspen_atomic, make}, {aspen_atomic, one}, {aspen_atomic, two},
     {aspen_keyword, {left, right}, {{aspen_atomic, first}, {aspen_atomic, second}}}]
        = Messages,
    _ = erlang:trace_pattern({aspen_runtime, call, 3}, false, [local]),
    _ = erlang:trace_pattern({aspen_runtime, send, 2}, false, [local]),
    ok = aspen_runtime:shutdown(Session).

collect_requests(Entry, Acc) ->
    receive
        {trace, Entry, call, {aspen_runtime, call, [_, Message, _]}} ->
            collect_requests(Entry, [Message | Acc]);
        {trace, Entry, call, {aspen_runtime, send, [_, Message]}} ->
            collect_requests(Entry, [Message | Acc]);
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
        {trace, Entry, call, {aspen_runtime, call, [_, Message, _]}} ->
            collect_delivered(Entry, Delivered, [Message | Acc]);
        {trace, Entry, call, {aspen_runtime, send, [_, Message]}} ->
            collect_delivered(Entry, Delivered, [Message | Acc]);
        {trace_delivered, Entry, Delivered} -> lists:reverse(Acc)
    after 3000 -> error(trace_delivery_timeout)
    end.

%% Observer PIDs below are deliberately sent through raw Erlang test messages,
%% not exported Aspen handles. They are used only to monitor reclamation.
garbage_collection() ->
    gc_globals(),
    gc_idle_cycle(),
    gc_blocked_call_with_mailbox(),
    gc_live_ping_pong(),
    gc_host_root(),
    gc_delegated_reply(),
    gc_automatic_collection(),
    gc_host_release(),
    gc_cross_session_export(),
    gc_cross_session_spawn_capture(),
    gc_late_reply_drops_actor_payload(),
    gc_duplicate_reply_drops_actor_payload(),
    ok.

gc_idle_cycle() ->
    Session = aspen_runtime:start_session(),
    Observer = self(),
    aspen_runtime:spawn_actor(Session, fun() ->
        A = aspen_runtime:spawn_actor(Session, fun() -> gc_peer_start(Session) end),
        B = aspen_runtime:spawn_actor(Session, fun() -> gc_peer_start(Session) end),
        aspen_runtime:send(A, {peer, B}),
        aspen_runtime:send(B, {peer, A}),
        Observer ! {idle_cycle, A, B}
    end),
    receive {idle_cycle, A, B} ->
        gc_await_dead(Session, [A, B], 200)
    after 2000 -> error(gc_cycle_setup_timeout) end,
    ok = aspen_runtime:shutdown(Session).

gc_peer_start(Session) ->
    {aspen_request, {peer, Peer}, none} = aspen_runtime:receive_request(Session, []),
    gc_idle(Session, [Peer]).

gc_idle(Session, Captures) ->
    {aspen_request, _, _} = aspen_runtime:receive_request(Session, Captures),
    gc_idle(Session, Captures).

gc_blocked_call_with_mailbox() ->
    Session = aspen_runtime:start_session(),
    Observer = self(),
    aspen_runtime:spawn_actor(Session, fun() ->
        Target = aspen_runtime:spawn_actor(Session, fun() -> gc_idle(Session, []) end),
        Caller = aspen_runtime:spawn_actor(Session, fun() ->
            {aspen_request, {go, Callee}, none} =
                aspen_runtime:receive_request(Session, []),
            aspen_runtime:call(Callee, ignored, []),
            error(impossible_call_returned)
        end),
        aspen_runtime:send(Caller, {go, Target}),
        aspen_runtime:send(Caller, queued_ordinary_request),
        Observer ! {blocked_call, Caller, Target}
    end),
    receive {blocked_call, Caller, Target} ->
        gc_await_dead(Session, [Caller, Target], 200)
    after 2000 -> error(gc_blocked_setup_timeout) end,
    ok = aspen_runtime:shutdown(Session).

gc_live_ping_pong() ->
    Session = aspen_runtime:start_session(),
    Observer = self(),
    aspen_runtime:spawn_actor(Session, fun() ->
        A = aspen_runtime:spawn_actor(Session, fun() -> gc_ping(Session, Observer, 0) end),
        B = aspen_runtime:spawn_actor(Session, fun() -> gc_ping(Session, Observer, 0) end),
        aspen_runtime:send(A, {ping, B}),
        Observer ! {ping_pong, A, B}
    end),
    receive {ping_pong, A, B} ->
        lists:foreach(fun(_) ->
            _ = aspen_runtime:collect(Session),
            true = is_process_alive(A),
            true = is_process_alive(B)
        end, lists:seq(1, 30)),
        %% Discard old progress reports, then require work after collection.
        gc_flush_ticks(),
        receive gc_tick -> ok after 2000 -> error(gc_live_cycle_stalled) end
    after 2000 -> error(gc_ping_setup_timeout) end,
    ok = aspen_runtime:shutdown(Session),
    gc_flush_ticks().

gc_ping(Session, Observer, Count) ->
    {aspen_request, {ping, Peer}, none} =
        aspen_runtime:receive_request(Session, [Observer]),
    aspen_runtime:send(Peer, {ping, self()}),
    case Count rem 100 of 0 -> Observer ! gc_tick; _ -> ok end,
    gc_ping(Session, Observer, Count + 1).

gc_flush_ticks() ->
    receive gc_tick -> gc_flush_ticks() after 0 -> ok end.

gc_host_root() ->
    Session = aspen_runtime:start_session(),
    Actor = aspen_runtime:spawn_actor(Session, fun() -> gc_idle(Session, []) end),
    lists:foreach(fun(_) ->
        _ = aspen_runtime:collect(Session),
        true = is_process_alive(Actor)
    end, lists:seq(1, 10)),
    ok = aspen_runtime:shutdown(Session).

gc_await_dead(_, Pids, 0) ->
    error({gc_not_collected, [Pid || Pid <- Pids, is_process_alive(Pid)]});
gc_await_dead(Session, Pids, Attempts) ->
    _ = aspen_runtime:collect(Session),
    case [Pid || Pid <- Pids, is_process_alive(Pid)] of
        [] -> ok;
        _ ->
            receive after 1 -> ok end,
            gc_await_dead(Session, Pids, Attempts - 1)
    end.

gc_delegated_reply() ->
    Session = aspen_runtime:start_session(),
    Observer = self(),
    aspen_runtime:spawn_actor(Session, fun() ->
        Server = aspen_runtime:spawn_actor(Session, fun() ->
            {aspen_request, go, Reply} = aspen_runtime:receive_request(Session, []),
            aspen_runtime:spawn_actor(Session, fun() ->
                Observer ! {gc_delegate, self()},
                receive release_reply -> ok after 2000 -> error(gc_delegate_timeout) end,
                Value = aspen_runtime:spawn_actor(Session, fun() ->
                    {aspen_request, check, ValueReply} =
                        aspen_runtime:receive_request(Session, []),
                    aspen_runtime:send(ValueReply, value_alive)
                end),
                aspen_runtime:send(Reply, {nested, {value, Value}})
            end)
        end),
        Caller = aspen_runtime:spawn_actor(Session, fun() ->
            {nested, {value, Value}} = aspen_runtime:call(Server, go, [Observer]),
            value_alive = aspen_runtime:call(Value, check, [Observer]),
            Observer ! gc_delegated_complete
        end),
        Observer ! {gc_delegated_actors, Server, Caller}
    end),
    receive {gc_delegated_actors, Server, Caller} ->
        receive {gc_delegate, Delegate} ->
            ServerMonitor = monitor(process, Server),
            receive {'DOWN', ServerMonitor, process, Server, _} -> ok
            after 2000 -> error(gc_original_server_alive) end,
            lists:foreach(fun(_) ->
                _ = aspen_runtime:collect(Session),
                true = is_process_alive(Caller),
                true = is_process_alive(Delegate)
            end, lists:seq(1, 10)),
            Delegate ! release_reply,
            receive gc_delegated_complete -> ok
            after 2000 -> error(gc_delegated_reply_lost) end
        after 2000 -> error(gc_delegate_setup_timeout) end
    after 2000 -> error(gc_delegated_setup_timeout) end,
    ok = aspen_runtime:shutdown(Session).

gc_automatic_collection() ->
    Session = aspen_runtime:start_session(),
    Observer = self(),
    aspen_runtime:spawn_actor(Session, fun() ->
        Actor = aspen_runtime:spawn_actor(Session, fun() -> gc_idle(Session, []) end),
        Observer ! {gc_automatic, Actor}
    end),
    receive {gc_automatic, Actor} ->
        Monitor = monitor(process, Actor),
        %% No explicit collection: the runtime's periodic tick must suffice.
        receive {'DOWN', Monitor, process, Actor, _} -> ok
        after 2000 -> error(gc_automatic_collection_timeout) end
    after 2000 -> error(gc_automatic_setup_timeout) end,
    ok = aspen_runtime:shutdown(Session).

gc_host_release() ->
    Session = aspen_runtime:start_session(),
    Actor = aspen_runtime:spawn_actor(Session, fun() -> gc_idle(Session, []) end),
    _ = aspen_runtime:collect(Session),
    true = is_process_alive(Actor),
    ok = aspen_runtime:unpin(Session, Actor),
    gc_await_dead(Session, [Actor], 200),
    ok = aspen_runtime:shutdown(Session).

gc_cross_session_export() ->
    Source = aspen_runtime:start_session(),
    Destination = aspen_runtime:start_session(),
    Observer = self(),
    Receiver = aspen_runtime:spawn_actor(Destination, fun() ->
        {aspen_request, {nested, #{actor := Value}}, none} =
            aspen_runtime:receive_request(Destination, []),
        Observer ! {gc_export_received, self(), Value},
        receive check_export -> ok after 2000 -> error(gc_export_gate_timeout) end,
        export_alive = aspen_runtime:call(Value, check, [Observer]),
        Observer ! gc_export_complete
    end),
    aspen_runtime:spawn_actor(Source, fun() ->
        Value = aspen_runtime:spawn_actor(Source, fun() ->
            {aspen_request, check, Reply} = aspen_runtime:receive_request(Source, []),
            aspen_runtime:send(Reply, export_alive)
        end),
        aspen_runtime:send(Receiver, {nested, #{actor => Value}})
    end),
    receive {gc_export_received, Receiver, Value} ->
        lists:foreach(fun(_) ->
            _ = aspen_runtime:collect(Source),
            _ = aspen_runtime:collect(Destination),
            true = is_process_alive(Value)
        end, lists:seq(1, 10)),
        Receiver ! check_export,
        receive gc_export_complete -> ok
        after 2000 -> error(gc_cross_session_reply_lost) end
    after 2000 -> error(gc_export_setup_timeout) end,
    ok = aspen_runtime:shutdown(Source),
    ok = aspen_runtime:shutdown(Destination).

gc_late_reply_drops_actor_payload() ->
    Session = aspen_runtime:start_session(),
    Observer = self(),
    Server = aspen_runtime:spawn_actor(Session, fun() ->
        {aspen_request, go, Reply} = aspen_runtime:receive_request(Session, []),
        aspen_runtime:send(Reply, first),
        %% The host opens this gate only after consuming and retiring the alias.
        receive send_late -> ok after 2000 -> error(gc_late_gate_timeout) end,
        Value = aspen_runtime:spawn_actor(Session, fun() -> gc_idle(Session, []) end),
        Observer ! {gc_late_value, Value},
        aspen_runtime:send(Reply, {nested, Value}),
        Observer ! gc_late_sent
    end),
    first = aspen_runtime:call(Server, go),
    Server ! send_late,
    receive {gc_late_value, Value} ->
        receive gc_late_sent -> ok after 2000 -> error(gc_late_send_timeout) end,
        gc_await_dead(Session, [Value], 200)
    after 2000 -> error(gc_late_setup_timeout) end,
    ok = aspen_runtime:shutdown(Session).

gc_cross_session_spawn_capture() ->
    Source = aspen_runtime:start_session(),
    Destination = aspen_runtime:start_session(),
    Observer = self(),
    Creator = aspen_runtime:spawn_actor(Source, fun() ->
        Value = aspen_runtime:spawn_actor(Source, fun() ->
            {aspen_request, check, Reply} = aspen_runtime:receive_request(Source, []),
            aspen_runtime:send(Reply, captured_alive)
        end),
        Remote = aspen_runtime:spawn_actor(Destination, fun() ->
            receive use_capture -> ok after 2000 -> error(gc_capture_gate_timeout) end,
            captured_alive = aspen_runtime:call(Value, check, [Observer]),
            Observer ! gc_capture_complete
        end),
        Observer ! {gc_remote_capture, Value, Remote}
    end),
    CreatorMonitor = monitor(process, Creator),
    receive {gc_remote_capture, Value, Remote} ->
        receive {'DOWN', CreatorMonitor, process, Creator, _} -> ok
        after 2000 -> error(gc_capture_creator_alive) end,
        lists:foreach(fun(_) ->
            _ = aspen_runtime:collect(Source),
            true = is_process_alive(Value)
        end, lists:seq(1, 10)),
        Remote ! use_capture,
        receive gc_capture_complete -> ok
        after 2000 -> error(gc_cross_session_capture_lost) end
    after 2000 -> error(gc_capture_setup_timeout) end,
    ok = aspen_runtime:shutdown(Source),
    ok = aspen_runtime:shutdown(Destination).

gc_duplicate_reply_drops_actor_payload() ->
    Session = aspen_runtime:start_session(),
    Observer = self(),
    Server = aspen_runtime:spawn_actor(Session, fun() ->
        {aspen_request, go, Reply} = aspen_runtime:receive_request(Session, []),
        Value = aspen_runtime:spawn_actor(Session, fun() -> gc_idle(Session, []) end),
        %% Both responses must be delivered before the host can consume either.
        true = erlang:suspend_process(Observer),
        try
            aspen_runtime:send(Reply, first),
            aspen_runtime:send(Reply, {duplicate, Value}),
            Observer ! {gc_duplicate_value, Value}
        after
            true = erlang:resume_process(Observer)
        end
    end),
    first = aspen_runtime:call(Server, go),
    receive {gc_duplicate_value, Value} ->
        gc_await_dead(Session, [Value], 200)
    after 2000 -> error(gc_duplicate_setup_timeout) end,
    receive {aspen_response, _, _} -> error(gc_duplicate_response_leaked)
    after 0 -> ok end,
    ok = aspen_runtime:shutdown(Session).

%% Global names are code edges, not permanent roots or lexical PID captures.
gc_globals() ->
    Session = aspen_runtime:start_session(),
    Observer = self(),
    Creator = aspen_runtime:spawn_actor(Session, fun() ->
        ok = aspen_runtime:initialize_globals(Session, fun() ->
            A = aspen_runtime:spawn_actor(Session,
                fun() -> gc_global_loop(Session, b) end, [b]),
            ok = aspen_runtime:define_global(Session, a, A),
            %% An active initializer must protect values already constructed.
            _ = aspen_runtime:collect(Session),
            true = is_process_alive(A),
            B = aspen_runtime:spawn_actor(Session,
                fun() -> gc_global_loop(Session, a) end, [a]),
            ok = aspen_runtime:define_global(Session, b, B),
            ok = aspen_runtime:define_global(Session, alias_a, A),
            ok = aspen_runtime:define_global(Session, wrapped, {nested, A})
        end),
        ok = aspen_runtime:initialize_globals(Session,
            fun() -> error(globals_initialized_twice) end),
        A = aspen_runtime:global(Session, a),
        A = aspen_runtime:global(Session, alias_a),
        {nested, A} = aspen_runtime:global(Session, wrapped),
        B = aspen_runtime:global(Session, b),
        %% Only a symbolic edge to a selector keeps the mutually recursive pair.
        Root = aspen_runtime:spawn_actor(Session,
            fun() -> gc_idle(Session, []) end, [wrapped]),
        ok = aspen_runtime:pin(Session, Root),
        Observer ! {gc_globals_ready, A, B, Root}
    end),
    Monitor = monitor(process, Creator),
    receive {gc_globals_ready, A, B, Root} ->
        receive {'DOWN', Monitor, process, Creator, _} -> ok
        after 2000 -> error(gc_globals_creator_timeout) end,
        lists:foreach(fun(_) ->
            _ = aspen_runtime:collect(Session),
            true = is_process_alive(A),
            true = is_process_alive(B)
        end, lists:seq(1, 10)),
        %% A host lookup is an explicit export; release it before reclamation.
        A = aspen_runtime:global(Session, alias_a),
        B = aspen_runtime:call(A, other),
        A = aspen_runtime:call(B, other),
        ok = aspen_runtime:unpin(Session, A),
        ok = aspen_runtime:unpin(Session, B),
        ok = aspen_runtime:unpin(Session, Root),
        gc_await_dead(Session, [A, B, Root], 200)
    after 2000 -> error(gc_globals_setup_timeout) end,
    ok = aspen_runtime:shutdown(Session).

gc_global_loop(Session, Other) ->
    {aspen_request, other, Reply} = aspen_runtime:receive_request(Session, []),
    aspen_runtime:send(Reply, aspen_runtime:global(Session, Other)),
    gc_global_loop(Session, Other).
