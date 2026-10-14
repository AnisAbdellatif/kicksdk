defmodule Kicksdk.Realtime.Session do
  @moduledoc """
  One connection's worth of Kick's Centrifugo protocol, as a state machine.

  Tell it what happens and do what it says. Each call returns a list of
  actions, maps with an `"action"`:

    * `"send"` - send `"text"` as a text frame.
    * `"close"` - close the socket (`"reason"` says why), call `closed/1`, and
      reconnect with backoff if the connection is still wanted.
    * `"refresh_token"` - fetch a new token for the same client id and pass it
      to `refresh/3`. If that fails, do nothing: Kick ends the connection when
      the old one runs out.
    * `"connected"`, `"subscribed"`, `"unsubscribed"`, `"subscribe_failed"` -
      the connection's state.
    * `"event"` - with `"channel"` (the feed), `"name"` (Kick's event name),
      `"data"` (as Kick sent it) and `"event"` (typed, as in `Kicksdk.Events`).

  The session reads no clock and sets no timer: pass `now` (milliseconds,
  monotonic is fine) to every call, and call `tick/2` when `deadline/1` comes.

      session = Session.new(Kicksdk.Realtime.chat_channels(chatroom_id, channel_id))
      # socket opened
      Session.open(session, token, now)
      # each text frame
      Session.receive(session, frame, now)

  The session is a reference to native state; it is not safe to share between
  processes that call it concurrently in a meaningful order. Keep it in the
  process that owns the socket.

  If a call ever panicked inside the NIF (a bug), the session's state can no
  longer be trusted: every call but `closed/1` raises `RuntimeError` until
  `closed/1` resets it.

  A frame's lines that are not UTF-8 are skipped, like lines that are not
  JSON. A token or feed name that is not UTF-8 raises `ArgumentError`.
  """

  @type t :: reference()
  @type action :: %{required(String.t()) => term()}

  @doc "A session for these feeds."
  @spec new([String.t()]) :: t()
  def new(channels), do: Kicksdk.Native.session_new(channels)

  @doc "The socket opened."
  @spec open(t(), String.t(), non_neg_integer()) :: [action()]
  def open(s, token, now), do: Kicksdk.Native.session_open(s, token, now)

  @doc "A text frame arrived."
  @spec receive(t(), String.t(), non_neg_integer()) :: [action()]
  def receive(s, text, now), do: Kicksdk.Native.session_receive(s, text, now)

  @doc "A new token, after a `\"refresh_token\"` action."
  @spec refresh(t(), String.t(), non_neg_integer()) :: [action()]
  def refresh(s, token, now), do: Kicksdk.Native.session_refresh(s, token, now)

  @doc "The clock reached `deadline/1`."
  @spec tick(t(), non_neg_integer()) :: [action()]
  def tick(s, now), do: Kicksdk.Native.session_tick(s, now)

  @doc "When to call `tick/2` next, or nil."
  @spec deadline(t()) :: non_neg_integer() | nil
  def deadline(s), do: Kicksdk.Native.session_deadline(s)

  @doc "The socket closed: reports every feed as unsubscribed, and readies the session for `open/3`."
  @spec closed(t()) :: [action()]
  def closed(s), do: Kicksdk.Native.session_closed(s)

  @doc "Whether a feed is subscribed now."
  @spec subscribed?(t(), String.t()) :: boolean()
  def subscribed?(s, channel), do: Kicksdk.Native.session_subscribed(s, channel)

  @doc "Whether Kick accepted the connection."
  @spec connected?(t()) :: boolean()
  def connected?(s), do: Kicksdk.Native.session_connected(s)
end
