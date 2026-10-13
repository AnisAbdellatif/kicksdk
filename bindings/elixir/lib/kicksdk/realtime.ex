defmodule Kicksdk.Realtime do
  @moduledoc """
  Kick's realtime chat feed (Centrifugo).

  Connecting is three steps, all of which Kick only allows from
  `https://kick.com` - send `Origin: https://kick.com` on the WebSocket
  handshake (the HTTP requests here already carry it):

    1. `chat_connection_request/3` (or `connection_request/2`) and
       `parse_connection/2`: the server's URL. Each channel's chat lives in
       one region.
    2. `token_request/2` and `parse_token/2`: a guest token for a client id you
       make up (a UUID). It lasts 45 minutes; refresh it for the **same** id.
    3. Open the WebSocket and drive a `Kicksdk.Realtime.Session`.
  """

  import Kicksdk, only: [encode_endpoints: 1, request: 1]

  @origin "https://kick.com"

  @doc "The Origin Kick's realtime server accepts."
  def required_origin, do: @origin

  @doc """
  The feeds Kick's page joins for one chat, the chat feed (`chatrooms.<id>.v2`)
  first: chat, moderation, subs and hosts arriving; `chatrooms.<id>` (hosts
  again, another shape); `chatroom_<id>` (gifted subs, rewards); and with a
  channel id, `channel.<id>` (live/offline, hosts leaving) and `channel_<id>`
  (Kicks).
  """
  @spec chat_channels(pos_integer(), pos_integer() | nil) :: [String.t()]
  def chat_channels(chatroom_id, channel_id \\ nil), do: Kicksdk.Native.chat_channels(chatroom_id, channel_id)

  @doc "Where a channel's chat is served."
  def chat_connection_request(channel_id, client_id, endpoints \\ nil),
    do: request(Kicksdk.Native.chat_connection_request(encode_endpoints(endpoints), channel_id, client_id))

  @doc "Where Kick's realtime is served in general."
  def connection_request(client_id, endpoints \\ nil),
    do: request(Kicksdk.Native.connection_request(encode_endpoints(endpoints), client_id))

  @doc "Reads a connection answer: `{:ok, [%{\"provider\" => \"centrifugo\", \"url\" => url} | …]}`."
  def parse_connection(status, body), do: Kicksdk.Native.parse_connection(status, body)

  @doc "A guest token request for `client_id`."
  def token_request(client_id, endpoints \\ nil),
    do: request(Kicksdk.Native.token_request(encode_endpoints(endpoints), client_id))

  @doc "The token from a token answer."
  def parse_token(status, body), do: Kicksdk.Native.parse_token(status, body)
end
