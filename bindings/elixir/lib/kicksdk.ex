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
  "headers" => [{name, value}], "body" => binary | nil}`.

  One convention throughout:

    * A builder that cannot fail returns the request (`Kicksdk.Realtime.token_request/2`).
    * A builder that can refuse its arguments (a slug that is `..`, a token
      with a line break, a timeout over a week, text that is not UTF-8)
      returns `{:ok, request}` or `{:error, reason}`, as do parsers and
      `call/2`.
    * `reason` is `{:status, code, excerpt}` (Kick answered an error status),
      `{:shape, what}`, `{:json, message}`, `{:invalid, what}` (refused
      before sending), `{:webhook, message}` (a delivery that does not
      verify) or `{:call, message}` (`call/2` arguments that do not fit).
    * An argument no call could use - endpoints with an unknown key or a
      value that is not a URL string, text that is not UTF-8 where a plain
      value is returned - raises `ArgumentError`.

  Every builder takes an optional `endpoints` map last, to point any part of
  Kick somewhere else (a simulator, a proxy): `%{api: …, id: …, website: …,
  realtime: …}`, any subset (`nil` for Kick's own). Any other key raises:
  a typo must not quietly send the request to kick.com.

  Integers come back exact up to 64 bits. Kick sends no bigger ones; one in
  its data would come back as a float, as the Rust core reads it.
  """

  @typedoc "A request for your HTTP client."
  @type request :: %{required(String.t()) => term()}

  @typedoc "Why a call failed."
  @type reason ::
          {:status, non_neg_integer(), String.t()}
          | {:shape, String.t()}
          | {:json, String.t()}
          | {:invalid, String.t()}
          | {:webhook, String.t()}
          | {:call, String.t()}

  @typedoc "`{:ok, value}` or `{:error, reason}`."
  @type result(value) :: {:ok, value} | {:error, reason()}

  @typedoc "Where Kick is: any subset of the four, or nil for Kick itself."
  @type endpoints :: %{optional(:api | :id | :website | :realtime) => String.t()} | nil

  @doc false
  # Requests come out of the NIF and dispatch with headers as two-element
  # lists; they go out with tuples.
  def request({:ok, r}), do: {:ok, request(r)}
  def request({:error, _} = e), do: e
  def request(list) when is_list(list), do: Enum.map(list, &request/1)

  def request(%{"method" => _, "url" => _, "headers" => headers} = r) when is_list(headers),
    do: %{r | "headers" => Enum.map(headers, fn [k, v] -> {k, v} end)}

  def request(other), do: other

  @doc """
  Calls any core function by name (`kicksdk::dispatch`), e.g.
  `Kicksdk.call("website.channel_request", %{slug: "somechannel"})`. The modules here
  wrap the common ones; this reaches every one.

  Arguments are encoded as JSON, so their text must be UTF-8 (`webhooks.*`
  take a body that is not as `body_base64`); anything that does not encode is
  `{:error, {:call, message}}`. A request in the result has its headers as
  `{name, value}` tuples, as everywhere else.
  """
  @spec call(String.t(), map()) :: result(term())
  def call(name, args \\ %{}) when is_binary(name) and is_map(args) do
    case encode(args) do
      {:ok, json} -> request(Kicksdk.Native.call(name, json))
      {:error, _} = e -> e
    end
  end

  defp encode(args) do
    {:ok, JSON.encode!(args)}
  rescue
    e -> {:error, {:call, "the arguments do not encode as JSON: " <> describe(e)}}
  end

  defp describe(%ErlangError{original: {:invalid_byte, byte}}), do: "a string is not UTF-8 (byte #{byte})"
  defp describe(e), do: Exception.message(e)

  @doc false
  # A request builder through call/2.
  def call_request(name, args, endpoints) do
    case endpoints!(endpoints) do
      e when map_size(e) == 0 -> call(name, args)
      e -> call(name, Map.put(args, :endpoints, e))
    end
  end

  @doc false
  def encode_endpoints(endpoints) do
    case endpoints!(endpoints) do
      e when map_size(e) == 0 -> ""
      e -> JSON.encode!(e)
    end
  end

  @endpoint_keys ~w(api id website realtime)

  # The override checked, with string keys and without nil values.
  defp endpoints!(nil), do: %{}

  defp endpoints!(%{} = endpoints) do
    for {key, url} <- endpoints, url != nil, into: %{} do
      name = if is_atom(key) or is_binary(key), do: to_string(key)

      if name not in @endpoint_keys,
        do:
          raise(
            ArgumentError,
            "endpoints: unknown key #{inspect(key)} (expected :api, :id, :website, :realtime)"
          )

      if not (is_binary(url) and String.valid?(url)),
        do: raise(ArgumentError, "endpoints: #{inspect(key)} must be a URL string, got: #{inspect(url)}")

      {name, url}
    end
  end

  defp endpoints!(other), do: raise(ArgumentError, "endpoints must be a map or nil, got: #{inspect(other)}")
end
