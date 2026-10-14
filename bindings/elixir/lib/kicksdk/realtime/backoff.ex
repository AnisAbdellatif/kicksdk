defmodule Kicksdk.Realtime.Backoff do
  @moduledoc """
  How long to wait before reconnecting. Pure arithmetic, from the Rust core;
  the timer is yours.

  Two stages, both ended by `reset/1` (call it when the chat feed is
  subscribed):

    * ordinary drops: 1 s, doubling to 30 s;
    * terminal closes (a refused token, a ban, a connection limit - a
      `"close"` action with `"terminal"` true, or a socket close code that
      `Kicksdk.Realtime.retry_after_ms/1` gives more than 0 for): at least
      the close's `retry_after_ms` (5 minutes from Kick), doubling on each
      terminal close in a row to six times it (5, 10, 20, 30, 30 …
      minutes), plus up to a quarter more.

  `next_delay_for/3` picks the stage from the close's `retry_after_ms`:

      {delay, backoff} = Backoff.next_delay_for(backoff, close["retry_after_ms"], :rand.uniform())
      Process.send_after(self(), :reconnect, delay)

  The random number (uniform in `[0, 1)`, `:rand.uniform/0`) spreads the
  waits, so that clients dropped together do not come back together.

  A value, not a process: each call returns the next state, which carries
  the terminal count - keep the returned struct, or the escalation is lost.
  """

  @enforce_keys [:initial_ms, :max_ms, :next_ms]
  defstruct [:initial_ms, :max_ms, :next_ms, terminal_closes: 0]

  @type t :: %__MODULE__{
          initial_ms: pos_integer(),
          max_ms: pos_integer(),
          next_ms: pos_integer(),
          terminal_closes: non_neg_integer()
        }

  @doc """
  A backoff from `initial_ms`, doubling to `max_ms`. A first wait of 0 counts
  as 1 ms, and a longest wait below the first is raised to it.
  """
  @spec new(non_neg_integer(), non_neg_integer()) :: t()
  def new(initial_ms \\ 1_000, max_ms \\ 30_000)
      when is_integer(initial_ms) and initial_ms >= 0 and is_integer(max_ms) and max_ms >= 0 do
    from(Kicksdk.Native.backoff_new(initial_ms, max_ms))
  end

  @doc "How long to wait before the next attempt, ms; the one after waits longer."
  @spec next_delay(t()) :: {pos_integer(), t()}
  def next_delay(%__MODULE__{} = b), do: delay(Kicksdk.Native.backoff_next(fields(b), nil))

  @doc """
  `next_delay/1`, spread from half of it to all of it by `random`, a number
  drawn uniformly from `[0, 1)` (`:rand.uniform/0`). Out of range is clamped.
  """
  @spec next_delay_jittered(t(), number()) :: {pos_integer(), t()}
  def next_delay_jittered(%__MODULE__{} = b, random) when is_number(random),
    do: delay(Kicksdk.Native.backoff_next(fields(b), random / 1))

  @doc """
  How long to wait after a close that asks for at least `retry_after_ms`: a
  `"close"` action's `"retry_after_ms"`, or
  `Kicksdk.Realtime.retry_after_ms/1` of the code the socket closed with.

    * `0` (an ordinary drop, or a failure with no close code): exactly
      `next_delay_jittered/2`.
    * More (a terminal close): `retry_after_ms`, doubled for each terminal
      close before this one since the last `reset/1`, at most six times it,
      plus `random` times a quarter of that. The ordinary stage is left as
      it was.

  With Kick's 300_000 and `random` 0: 300_000, 600_000, 1_200_000,
  1_800_000, 1_800_000 … ms. `random` is drawn uniformly from `[0, 1)`; out
  of range is clamped.
  """
  @spec next_delay_for(t(), non_neg_integer(), number()) :: {pos_integer(), t()}
  def next_delay_for(%__MODULE__{} = b, retry_after_ms, random)
      when is_integer(retry_after_ms) and retry_after_ms >= 0 and is_number(random),
      do: delay(Kicksdk.Native.backoff_next_for(fields(b), retry_after_ms, random / 1))

  @doc """
  A connection worked (the chat feed is subscribed): the next failure starts
  from the beginning, and so does the next terminal close.
  """
  @spec reset(t()) :: t()
  def reset(%__MODULE__{} = b), do: from(Kicksdk.Native.backoff_reset(fields(b)))

  defp delay({delay, next}), do: {delay, from(next)}

  defp from({i, m, n, t}), do: %__MODULE__{initial_ms: i, max_ms: m, next_ms: n, terminal_closes: t}

  defp fields(%__MODULE__{initial_ms: i, max_ms: m, next_ms: n, terminal_closes: t}), do: {i, m, n, t}
end
