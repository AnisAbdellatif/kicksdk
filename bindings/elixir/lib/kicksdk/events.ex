defmodule Kicksdk.Events do
  @moduledoc """
  Kick's chat-feed events, typed.

  `parse/2` takes an event name and its `data` as Kick sent it (the JSON text
  inside the envelope) and returns a map with a `"type"`: `"chat_message"`,
  `"message_deleted"`, `"user_banned"`, `"host"`, … An event Kicksdk does not
  know comes back as `%{"type" => "unknown", "name" => name}` rather than an
  error.

  A `Kicksdk.Realtime.Session` already does this for every event it receives.
  """

  alias Kicksdk.Native

  @doc """
  Reads one event. `data_json` that is not JSON (or not UTF-8) is read as
  text, which gives an event with what could be read; the name must be
  UTF-8 (else `ArgumentError`).
  """
  @spec parse(String.t(), binary()) :: map()
  def parse(name, data_json),
    do:
      if(Native.dirty?(data_json),
        do: Native.parse_event_dirty(name, data_json),
        else: Native.parse_event(name, data_json)
      )

  @doc ~s(Splits message text into `"text"` and `"emote"` parts. `ArgumentError` if it is not UTF-8.)
  @spec parse_content(String.t(), boolean()) :: [map()]
  def parse_content(text, combine \\ false),
    do:
      if(Native.dirty?(text),
        do: Native.parse_content_dirty(text, combine),
        else: Native.parse_content(text, combine)
      )

  @doc "An emote's image URL: `{:ok, url}`, or `{:error, {:invalid, what}}` for an id that is not digits."
  @spec emote_url(String.t()) :: Kicksdk.result(String.t())
  def emote_url(id), do: Native.emote_url(id)
end
