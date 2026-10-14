defmodule Kicksdk.PublicApi do
  @moduledoc """
  Kick's official API (`api.kick.com/public/v1`).

  An app token reads any channel's public data and subscribes to any channel's
  webhooks. Batches are 50 ids; one unknown slug fails a whole `/channels`
  request (400), so batch by id. `viewer_count` refreshes about once a minute.
  See `docs/KICK.md` §3.

  The builders that send a token refuse one with a control character (a
  line break would start a header of its own): they return `{:ok, request}`
  or `{:error, {:invalid, what}}`. The app token request is the exception,
  and returns the request.
  """

  import Kicksdk, only: [encode_endpoints: 1, request: 1]
  alias Kicksdk.Native

  @doc "An app token (client credentials)."
  @spec app_token_request(String.t(), String.t(), Kicksdk.endpoints()) :: Kicksdk.request()
  def app_token_request(client_id, secret, endpoints \\ nil),
    do: request(Native.app_token_request(encode_endpoints(endpoints), client_id, secret))

  @doc "Reads an app token answer."
  @spec parse_app_token(integer(), binary()) :: Kicksdk.result(map())
  def parse_app_token(status, body), do: Native.parse_app_token(status, body)

  @doc "`/livestreams` for any number of user ids: one request per 50."
  @spec livestreams_requests(String.t(), [non_neg_integer()], Kicksdk.endpoints()) ::
          Kicksdk.result([Kicksdk.request()])
  def livestreams_requests(token, user_ids, endpoints \\ nil),
    do: request(Native.livestreams_requests(encode_endpoints(endpoints), token, user_ids))

  @doc """
  The top live streams, overall or in a category or language: up to 100.
  Options: `:category_id`, `:language`, `:sort` (`"viewer_count"` or
  `"started_at"`), `:limit`, `:endpoints`.
  """
  @spec top_livestreams_request(String.t(), keyword()) :: Kicksdk.result(Kicksdk.request())
  def top_livestreams_request(token, opts \\ []) do
    args = %{token: token, category_id: opts[:category_id], language: opts[:language]}
    args = Map.merge(args, %{sort: opts[:sort] || "viewer_count", limit: opts[:limit] || 100})
    Kicksdk.call_request("public_api.top_livestreams_request", args, opts[:endpoints])
  end

  @doc "Reads a `/livestreams` answer. Offline is `{:ok, []}`."
  @spec parse_livestreams(integer(), binary()) :: Kicksdk.result([map()])
  def parse_livestreams(status, body), do: Native.parse_livestreams(status, body)

  @doc "`/channels` for up to 50 user ids."
  @spec channels_by_ids_request(String.t(), [non_neg_integer()], Kicksdk.endpoints()) ::
          Kicksdk.result(Kicksdk.request())
  def channels_by_ids_request(token, user_ids, endpoints \\ nil),
    do: request(Native.channels_by_ids_request(encode_endpoints(endpoints), token, user_ids))

  @doc "`/channels` for up to 50 slugs (one unknown slug fails them all)."
  @spec channels_by_slugs_request(String.t(), [String.t()], Kicksdk.endpoints()) ::
          Kicksdk.result(Kicksdk.request())
  def channels_by_slugs_request(token, slugs, endpoints \\ nil),
    do: request(Native.channels_by_slugs_request(encode_endpoints(endpoints), token, slugs))

  @doc "Reads a `/channels` answer."
  @spec parse_channels(integer(), binary()) :: Kicksdk.result([map()])
  def parse_channels(status, body), do: Native.parse_channels(status, body)

  @doc "The webhook signing key."
  @spec public_key_request(Kicksdk.endpoints()) :: Kicksdk.request()
  def public_key_request(endpoints \\ nil),
    do: request(Native.public_key_request(encode_endpoints(endpoints)))

  @doc "The PEM from a public key answer."
  @spec parse_public_key(integer(), binary()) :: Kicksdk.result(String.t())
  def parse_public_key(status, body), do: Native.parse_public_key(status, body)

  @doc "Subscribes the app to `events` (`[{name, version}]`) for one channel."
  @spec subscribe_request(
          String.t(),
          non_neg_integer(),
          [{String.t(), non_neg_integer()}],
          Kicksdk.endpoints()
        ) ::
          Kicksdk.result(Kicksdk.request())
  def subscribe_request(token, broadcaster_user_id, events, endpoints \\ nil),
    do: request(Native.subscribe_request(encode_endpoints(endpoints), token, broadcaster_user_id, events))

  @doc "Every webhook subscription the app has."
  @spec subscriptions_request(String.t(), Kicksdk.endpoints()) :: Kicksdk.result(Kicksdk.request())
  def subscriptions_request(token, endpoints \\ nil),
    do: request(Native.subscriptions_request(encode_endpoints(endpoints), token))

  @doc "Reads a subscriptions answer."
  @spec parse_subscriptions(integer(), binary()) :: Kicksdk.result([map()])
  def parse_subscriptions(status, body), do: Native.parse_subscriptions(status, body)

  @doc "Removes subscriptions, 50 per request."
  @spec unsubscribe_requests(String.t(), [String.t()], Kicksdk.endpoints()) ::
          Kicksdk.result([Kicksdk.request()])
  def unsubscribe_requests(token, ids, endpoints \\ nil),
    do: request(Native.unsubscribe_requests(encode_endpoints(endpoints), token, ids))
end
