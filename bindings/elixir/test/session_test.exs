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

    assert [%{"action" => "close", "code" => 0, "reconnect" => true}] = Session.tick(s, deadline)
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
