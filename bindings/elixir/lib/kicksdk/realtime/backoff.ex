defmodule Kicksdk.Realtime.Backoff do
  @moduledoc """
  How long to wait before reconnecting: 1 s, doubling to 30 s, back to 1 s
  once a connection has worked (`reset/1`, when the chat feed is
  subscribed). Pure arithmetic, from the Rust core; the timer is yours.

  Many clients dropped at once (a server restart) would all come back in
  step; `next_delay_jittered/2` spreads them with a random number you draw:

      {delay, backoff} = Backoff.next_delay_jittered(backoff, :rand.uniform())
      Process.send_after(self(), :reconnect, delay)

  A value, not a process: each call returns the next state.
  """

  @enforce_keys [:initial_ms, :max_ms, :next_ms]
  defstruct [:initial_ms, :max_ms, :next_ms]

  @type t :: %__MODULE__{
          initial_ms: pos_integer(),
          max_ms: pos_integer(),
          next_ms: pos_integer()
        }

  @doc """
  A backoff from `initial_ms`, doubling to `max_ms`. A first wait of 0 counts
  as 1 ms, and a longest wait below the first is raised to it.
  """
  @spec new(non_neg_integer(), non_neg_integer()) :: t()
  def new(initial_ms \\ 1_000, max_ms \\ 30_000)
      when is_integer(initial_ms) and initial_ms >= 0 and is_integer(max_ms) and max_ms >= 0 do
    {initial, max, next} = Kicksdk.Native.backoff_new(initial_ms, max_ms)
    %__MODULE__{initial_ms: initial, max_ms: max, next_ms: next}
  end

  @doc "How long to wait before the next attempt, ms; the one after waits longer."
  @spec next_delay(t()) :: {pos_integer(), t()}
  def next_delay(%__MODULE__{} = b) do
    {delay, next} = Kicksdk.Native.backoff_next(fields(b), nil)
    {delay, %{b | next_ms: next}}
  end

  @doc """
  `next_delay/1`, spread from half of it to all of it by `random`, a number
  drawn uniformly from `[0, 1)` (`:rand.uniform/0`). Out of range is clamped.
  """
  @spec next_delay_jittered(t(), number()) :: {pos_integer(), t()}
  def next_delay_jittered(%__MODULE__{} = b, random) when is_number(random) do
    {delay, next} = Kicksdk.Native.backoff_next(fields(b), random / 1)
    {delay, %{b | next_ms: next}}
  end

  @doc "A connection worked: the next failure starts from the beginning."
  @spec reset(t()) :: t()
  def reset(%__MODULE__{} = b), do: %{b | next_ms: Kicksdk.Native.backoff_reset(fields(b))}

  defp fields(%__MODULE__{initial_ms: i, max_ms: m, next_ms: n}), do: {i, m, n}
end
