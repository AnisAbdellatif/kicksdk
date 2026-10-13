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

  @doc "Reads one event."
  @spec parse(String.t(), String.t()) :: map()
  def parse(name, data_json), do: Kicksdk.Native.parse_event(name, data_json)

  @doc "Splits message text into `\"text\"` and `\"emote\"` parts."
  @spec parse_content(String.t(), boolean()) :: [map()]
  def parse_content(text, combine \\ false), do: Kicksdk.Native.parse_content(text, combine)

  @doc "An emote's image URL."
  @spec emote_url(String.t()) :: String.t()
  def emote_url(id), do: Kicksdk.Native.emote_url(id)
end
