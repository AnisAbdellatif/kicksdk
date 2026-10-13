defmodule Kicksdk do
  @moduledoc """
  Kick (kick.com) without the I/O.

  Kicksdk knows Kick - which requests to make, what the answers look like, how
  the realtime chat feed speaks, how webhooks are signed - and never sends a
  request or opens a socket itself. You do, with whatever HTTP and WebSocket
  client your application already uses, and hand Kicksdk what came back.

  The same Rust core runs in browsers (npm `kicksdk-js`) and here, as a NIF.

    * `Kicksdk.PublicApi` - the official API and the app token.
    * `Kicksdk.Website` - kick.com's own endpoints: chatroom id, follower totals, history.
    * `Kicksdk.Realtime` and `Kicksdk.Realtime.Session` - the chat feed.
    * `Kicksdk.Events` - chat-feed events, typed.
    * `Kicksdk.Webhooks` - signature verification and typed bodies.

  Values come back the way JSON decodes in Elixir: maps with **string keys**
  (`event["type"]`, `action["action"]`). Not atoms: most of it is Kick's own
  data, and atom keys from it would let Kick grow the atom table.

  A request is `%{"method" => "GET" | "POST" | "DELETE", "url" => url,
  "headers" => [{name, value}], "body" => binary | nil}`. A parser takes the
  status and body and returns `{:ok, value}` or `{:error, reason}`, where
  reason is `{:status, code, excerpt}`, `{:shape, what}`, `{:json, message}` or
  `{:invalid, what}`.

  Every function takes an optional `endpoints` map last, to point any part of
  Kick somewhere else (a simulator, a proxy): `%{api: …, id: …, website: …,
  realtime: …}`, any subset.
  """

  @doc false
  # Requests come out of the NIF with headers as two-element lists.
  def request({:ok, r}), do: {:ok, request(r)}
  def request({:error, _} = e), do: e
  def request(list) when is_list(list), do: Enum.map(list, &request/1)

  def request(%{"headers" => headers} = r),
    do: %{r | "headers" => Enum.map(headers, fn [k, v] -> {k, v} end)}

  @doc """
  Calls any core function by name (`kicksdk::dispatch`), e.g.
  `Kicksdk.call("website.channel_request", %{slug: "somechannel"})`. The modules here
  wrap the common ones; this reaches every one.
  """
  @spec call(String.t(), map()) :: {:ok, term()} | {:error, term()}
  def call(name, args \\ %{}), do: Kicksdk.Native.call(name, JSON.encode!(args))

  @doc false
  # A request builder through call/2: the request, or an error as raised.
  def call_request(name, args, endpoints) do
    args = if endpoints, do: Map.put(args, :endpoints, endpoints), else: args

    case call(name, args) do
      {:ok, r} -> request(r)
      {:error, reason} -> {:error, reason}
    end
  end

  @doc false
  def encode_endpoints(nil), do: ""
  def encode_endpoints(%{} = e) when map_size(e) == 0, do: ""
  def encode_endpoints(%{} = e), do: JSON.encode!(e)
end
