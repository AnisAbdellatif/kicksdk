defmodule Kicksdk.Realtime.BackoffTest do
  use ExUnit.Case, async: true

  alias Kicksdk.Realtime
  alias Kicksdk.Realtime.Backoff

  defp delays(b, n, fun) do
    Enum.map_reduce(1..n, b, fn _, b -> fun.(b) end) |> elem(0)
  end

  test "doubles to the cap and resets" do
    b = Backoff.new()
    assert delays(b, 7, &Backoff.next_delay/1) == [1_000, 2_000, 4_000, 8_000, 16_000, 30_000, 30_000]
    {_, b} = Backoff.next_delay(b)
    {_, b} = Backoff.next_delay(b)
    assert {1_000, _} = b |> Backoff.reset() |> Backoff.next_delay()
  end

  test "jitter spreads each delay over its upper half, by the caller's random number" do
    b = Backoff.new()
    assert delays(b, 3, &Backoff.next_delay_jittered(&1, 0.0)) == [500, 1_000, 2_000]
    assert delays(b, 3, &Backoff.next_delay_jittered(&1, 0.5)) == [750, 1_500, 3_000]

    for _ <- 1..100 do
      {d, _} = Backoff.next_delay_jittered(b, :rand.uniform())
      assert d in 500..1_000
    end

    # Out of range is clamped; an integer is a number too.
    assert {1_000, _} = Backoff.next_delay_jittered(b, 7)
    assert {500, _} = Backoff.next_delay_jittered(b, -1)
  end

  test "bounds are the core's: 0 is 1 ms, a max below the first is raised" do
    assert %Backoff{initial_ms: 1, max_ms: 1} = Backoff.new(0, 0)
    assert %Backoff{initial_ms: 500, max_ms: 500} = Backoff.new(500, 100)
  end

  test "close codes: Centrifugo's terminal ranges say do not reconnect" do
    for code <- [1000, 1006, 3000, 3499, 4000, 4499], do: assert(Realtime.should_reconnect?(code))
    for code <- [3500, 3501, 3999, 4500, 4999], do: refute(Realtime.should_reconnect?(code))
    assert_raise FunctionClauseError, fn -> Realtime.should_reconnect?(70_000) end
  end
end
