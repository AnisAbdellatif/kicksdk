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

  When the socket closes, reconnect with a `Kicksdk.Realtime.Backoff` unless
  the close says not to (`should_reconnect?/1`, or the session's `"close"`
  action's `"reconnect"`). `examples/elixir/chat.exs` does all of it.
  """

  import Kicksdk, only: [encode_endpoints: 1, request: 1]
  alias Kicksdk.Native

  @origin "https://kick.com"

  @doc "The Origin Kick's realtime server accepts."
  @spec required_origin() :: String.t()
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
  @spec chat_connection_request(non_neg_integer(), String.t(), Kicksdk.endpoints()) :: Kicksdk.request()
  def chat_connection_request(channel_id, client_id, endpoints \\ nil),
    do: request(Kicksdk.Native.chat_connection_request(encode_endpoints(endpoints), channel_id, client_id))

  @doc "Where Kick's realtime is served in general."
  @spec connection_request(String.t(), Kicksdk.endpoints()) :: Kicksdk.request()
  def connection_request(client_id, endpoints \\ nil),
    do: request(Kicksdk.Native.connection_request(encode_endpoints(endpoints), client_id))

  @doc ~s(Reads a connection answer: `{:ok, [%{"provider" => "centrifugo", "url" => url} | …]}`.)
  @spec parse_connection(integer(), binary()) :: Kicksdk.result([map()])
  def parse_connection(status, body),
    do:
      if(Native.dirty?(body),
        do: Native.parse_connection_dirty(status, body),
        else: Native.parse_connection(status, body)
      )

  @doc "A guest token request for `client_id`."
  @spec token_request(String.t(), Kicksdk.endpoints()) :: Kicksdk.request()
  def token_request(client_id, endpoints \\ nil),
    do: request(Kicksdk.Native.token_request(encode_endpoints(endpoints), client_id))

  @doc """
  Whether a connection that closed with this code is worth opening again, by
  Centrifugo's convention: false for 3500-3999 and 4500-4999 (the server does
  not want this client back as it is: a bad token, a ban), true for anything
  else - 3000-3499, 4000-4499, and the WebSocket codes below 3000 (a network
  drop is 1006). For the code your socket reports; a session's `"close"`
  action already carries the answer in `"reconnect"`.
  """
  @spec should_reconnect?(0..65_535) :: boolean()
  def should_reconnect?(code) when is_integer(code) and code in 0..65_535,
    do: Kicksdk.Native.should_reconnect(code)

  @doc "The token from a token answer."
  @spec parse_token(integer(), binary()) :: Kicksdk.result(String.t())
  def parse_token(status, body),
    do:
      if(Native.dirty?(body),
        do: Native.parse_token_dirty(status, body),
        else: Native.parse_token(status, body)
      )
end
