defmodule Kicksdk.Realtime.SessionTest do
  use ExUnit.Case, async: true

  alias Kicksdk.Realtime.Session

  @connected ~s({"id":1,"connect":{"client":"c","ping":25,"expires":true,"ttl":2700}})

  defp connected(now) do
    s = Session.new(["chatrooms.1.v2"])
    assert [%{"action" => "send"}] = Session.open(s, "t", now)
    assert [%{"action" => "connected"}, %{"action" => "send"}] = Session.receive(s, @connected, now + 10)
    s
  end

  test "runs on the BEAM's monotonic clock, negative as it usually is" do
    now = System.monotonic_time(:millisecond)
    s = connected(now)
    assert Session.connected?(s)

    # Kick pings every 25 s and is given 20 s more: silence until then is fine.
    deadline = Session.deadline(s)
    assert deadline == now + 10 + 45_000
    assert Session.tick(s, deadline - 1) == []

    assert [%{"action" => "close", "code" => 0, "terminal" => false, "retry_after_ms" => 0}] =
             Session.tick(s, deadline)
  end

  test "works on any integer clock, and asks for a token before it runs out" do
    for start <- [-(2 ** 62), -1, 0, 1_700_000_000_000] do
      s = connected(start)
      # Pings keep it alive; the token (45 minutes) is refreshed 2 minutes early.
      refresh_at = start + 10 + (2700 - 120) * 1000

      for t <- Range.new(start + 20_000, refresh_at - 1, 20_000),
          do: assert(Session.receive(s, "{}", t) == [%{"action" => "send", "text" => "{}"}])

      assert Session.deadline(s) <= refresh_at
      assert [%{"action" => "refresh_token"}] = Session.tick(s, refresh_at)
      assert [%{"action" => "send", "text" => text}] = Session.refresh(s, "t2", refresh_at + 5)
      assert text =~ ~s("refresh":{"token":"t2"})
    end
  end

  test "a close says how long to wait at least, and whether it was terminal" do
    s = connected(0)

    assert [
             %{
               "action" => "close",
               "code" => 3501,
               "reason" => reason,
               "terminal" => true,
               "retry_after_ms" => 300_000
             } = close
           ] = Session.receive(s, ~s({"push":{"disconnect":{"code":3501,"reason":"bad request"}}}), 20)

    assert reason =~ "3501"
    # Exactly these fields: "reconnect" is gone.
    assert Map.keys(close) |> Enum.sort() == ~w(action code reason retry_after_ms terminal)
    assert [%{"action" => "unsubscribed", "code" => 0}] = Session.closed(s)

    # Terminal is not the end: the session opens again after closed/1.
    assert [%{"action" => "send"}] = Session.open(s, "t2", 400_000)

    s = connected(0)

    assert [%{"action" => "close", "code" => 3005, "terminal" => false, "retry_after_ms" => 0}] =
             Session.receive(s, ~s({"push":{"disconnect":{"code":3005,"reason":"shutdown"}}}), 20)

    # A refused connect (101 unauthorized: a guest token Kick will not take).
    s = Session.new(["chatrooms.1.v2"])
    Session.open(s, "t", 0)

    assert [%{"action" => "close", "code" => 101, "terminal" => true, "retry_after_ms" => 300_000}] =
             Session.receive(s, ~s({"id":1,"error":{"code":101,"message":"unauthorized"}}), 5)
  end

  test "a session called from two processes at once raises instead of waiting" do
    s = connected(0)
    frame = String.duplicate("{}\n", 200_000)

    # Whichever call comes second raises: usually the poller, while the
    # big frame is read on a dirty scheduler.
    busy =
      Enum.find_value(1..20, fn _ ->
        task = Task.async(fn -> busy(fn -> Session.receive(s, frame, 20) end) end)
        seen = poll_busy(s, task)
        Task.await(task, 30_000) || seen
      end)

    assert busy =~ "busy"
    # Afterwards the session works as before.
    assert Session.connected?(s)
  end

  defp poll_busy(s, task) do
    case busy(fn -> Session.connected?(s) end) do
      nil -> if Process.alive?(task.pid), do: poll_busy(s, task)
      message -> message
    end
  end

  # nil, or the message of the RuntimeError the call raised.
  defp busy(call) do
    call.()
    nil
  rescue
    e in RuntimeError -> e.message
  end

  test "an earlier time than the first is not a wrap" do
    s = connected(1_000)
    assert Session.tick(s, -5_000) == []
    assert Session.connected?(s)
  end

  test "a clock that is not an integer is refused" do
    assert_raise ArgumentError, fn -> Session.open(Session.new([]), "t", 1.5) end
    assert_raise ArgumentError, fn -> Session.open(Session.new([]), "t", 2 ** 64) end
  end
end
