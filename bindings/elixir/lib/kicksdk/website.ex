defmodule Kicksdk.Website do
  @moduledoc """
  The endpoints kick.com's own frontend uses (`kick.com/api/v2`).

  Undocumented and a grey area under Kick's terms: treat a failure as a gap,
  never a zero. The only source of the **chatroom id** and the **follower
  total**. From a server, a bare `User-Agent: Mozilla/5.0` passes where a full
  browser one is refused ("Request blocked by security policy").
  """

  import Kicksdk, only: [encode_endpoints: 1, request: 1]
  alias Kicksdk.Native

  @doc "A channel by slug."
  def channel_request(slug, endpoints \\ nil),
    do: request(Native.channel_request(encode_endpoints(endpoints), slug))

  @doc "Reads a channel answer: chatroom id, followers (number or string upstream), live state."
  def parse_channel(status, body), do: Native.parse_channel(status, body)

  @doc "Recent chat and the pin, by channel id."
  def history_request(channel_id, endpoints \\ nil),
    do: request(Native.history_request(encode_endpoints(endpoints), channel_id))

  @doc "Reads a history answer: `%{\"messages\" => […], \"pinned\" => event | nil}`."
  def parse_history(status, body), do: Native.parse_history(status, body)

  @doc "A user as a channel sees them; a 404 means the channel does not know the name."
  def channel_user_request(slug, username, endpoints \\ nil),
    do: request(Native.channel_user_request(encode_endpoints(endpoints), slug, username))

  @doc "Reads a channel user answer."
  def parse_channel_user(status, body), do: Native.parse_channel_user(status, body)
end
