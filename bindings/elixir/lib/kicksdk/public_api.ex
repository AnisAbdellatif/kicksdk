defmodule Kicksdk.PublicApi do
  @moduledoc """
  Kick's official API (`api.kick.com/public/v1` and `/public/v2`).

  An app token reads any channel's public data and subscribes to any channel's
  webhooks. Batches are 50 ids (100 for live streams by user id); one unknown
  slug fails a whole `/channels` request (400), so batch by id. `viewer_count` refreshes about once a minute.
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
  def parse_app_token(status, body),
    do:
      if(Native.dirty?(body),
        do: Native.parse_app_token_dirty(status, body),
        else: Native.parse_app_token(status, body)
      )

  @doc "`/public/v1/users/livestreams` for any number of user ids: one request per 100."
  @spec livestreams_requests(String.t(), [non_neg_integer()], Kicksdk.endpoints()) ::
          Kicksdk.result([Kicksdk.request()])
  def livestreams_requests(token, user_ids, endpoints \\ nil),
    do: request(Native.livestreams_requests(encode_endpoints(endpoints), token, user_ids))

  @doc """
  One page of the streams live now (`/public/v2/livestreams`), in up to 25
  categories and 25 languages (none: all). Options: `:category_ids`,
  `:language_codes`, `:limit` (1 to 1000), `:cursor` (the previous page's
  `"next_cursor"`), `:endpoints`. Kick has no sort: streams come oldest first.
  """
  @spec livestreams_page_request(String.t(), keyword()) :: Kicksdk.result(Kicksdk.request())
  def livestreams_page_request(token, opts \\ []) do
    args = %{
      token: token,
      category_ids: opts[:category_ids],
      language_codes: opts[:language_codes],
      limit: opts[:limit],
      cursor: opts[:cursor]
    }

    Kicksdk.call_request("public_api.livestreams_page_request", args, opts[:endpoints])
  end

  @doc ~S|Reads a `livestreams_page_request/2` answer: `%{"data" => [...], "next_cursor" => ...}`.|
  @spec parse_livestreams_page(integer(), binary()) :: Kicksdk.result(map())
  def parse_livestreams_page(status, body),
    do: Kicksdk.call("public_api.parse_livestreams_page", %{status: status, body: body})

  @doc "Reads a `livestreams_requests/3` answer (or a page's streams alone). Offline is `{:ok, []}`."
  @spec parse_livestreams(integer(), binary()) :: Kicksdk.result([map()])
  def parse_livestreams(status, body),
    do:
      if(Native.dirty?(body),
        do: Native.parse_livestreams_dirty(status, body),
        else: Native.parse_livestreams(status, body)
      )

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
  def parse_channels(status, body),
    do:
      if(Native.dirty?(body),
        do: Native.parse_channels_dirty(status, body),
        else: Native.parse_channels(status, body)
      )

  @doc "The webhook signing key."
  @spec public_key_request(Kicksdk.endpoints()) :: Kicksdk.request()
  def public_key_request(endpoints \\ nil),
    do: request(Native.public_key_request(encode_endpoints(endpoints)))

  @doc "The PEM from a public key answer."
  @spec parse_public_key(integer(), binary()) :: Kicksdk.result(String.t())
  def parse_public_key(status, body),
    do:
      if(Native.dirty?(body),
        do: Native.parse_public_key_dirty(status, body),
        else: Native.parse_public_key(status, body)
      )

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
  def parse_subscriptions(status, body),
    do:
      if(Native.dirty?(body),
        do: Native.parse_subscriptions_dirty(status, body),
        else: Native.parse_subscriptions(status, body)
      )

  @doc "Removes subscriptions, 50 per request."
  @spec unsubscribe_requests(String.t(), [String.t()], Kicksdk.endpoints()) ::
          Kicksdk.result([Kicksdk.request()])
  def unsubscribe_requests(token, ids, endpoints \\ nil),
    do: request(Native.unsubscribe_requests(encode_endpoints(endpoints), token, ids))

  @doc """
  Users by id, up to 50 (`/public/v1/users`), with an app or a user token. No
  ids, with a user token: the token's own user.
  """
  @spec users_request(String.t(), [non_neg_integer()] | nil, Kicksdk.endpoints()) ::
          Kicksdk.result(Kicksdk.request())
  def users_request(token, user_ids \\ nil, endpoints \\ nil),
    do: Kicksdk.call_request("public_api.users_request", %{token: token, user_ids: user_ids}, endpoints)

  @doc ~s[Reads a users answer: `"user_id"`, `"name"`, `"profile_picture"`, `"email"` (only for a user token with `user:read`, for its own user).]
  @spec parse_users(integer(), binary()) :: Kicksdk.result([map()])
  def parse_users(status, body), do: Kicksdk.call("public_api.parse_users", %{status: status, body: body})

  @doc """
  One page of categories (`/public/v2/categories`): by `:names` and `:tags`,
  or by `:ids`; none of them, all categories. Other options: `:limit` (1 to
  1000; Kick's default: 25), `:cursor` (the previous page's `"next_cursor"`),
  `:endpoints`.
  """
  @spec categories_request(String.t(), keyword()) :: Kicksdk.result(Kicksdk.request())
  def categories_request(token, opts \\ []) do
    args = %{
      token: token,
      names: opts[:names],
      tags: opts[:tags],
      ids: opts[:ids],
      limit: opts[:limit],
      cursor: opts[:cursor]
    }

    Kicksdk.call_request("public_api.categories_request", args, opts[:endpoints])
  end

  @doc ~S|Reads a `categories_request/2` answer: `%{"data" => [...], "next_cursor" => ...}`.|
  @spec parse_categories(integer(), binary()) :: Kicksdk.result(map())
  def parse_categories(status, body),
    do: Kicksdk.call("public_api.parse_categories", %{status: status, body: body})

  @doc "How many streams are live on Kick, with an app or a user token."
  @spec livestream_stats_request(String.t(), Kicksdk.endpoints()) :: Kicksdk.result(Kicksdk.request())
  def livestream_stats_request(token, endpoints \\ nil),
    do: Kicksdk.call_request("public_api.livestream_stats_request", %{token: token}, endpoints)

  @doc ~s(Reads a stats answer: `%{"total_count" => n}`.)
  @spec parse_livestream_stats(integer(), binary()) :: Kicksdk.result(map())
  def parse_livestream_stats(status, body),
    do: Kicksdk.call("public_api.parse_livestream_stats", %{status: status, body: body})

  @doc """
  The top `top` Kicks gifters (1 to 100; Kick's default: 10) of the token's
  own channel. A user token with `kicks:read`.
  """
  @spec kicks_leaderboard_request(String.t(), pos_integer() | nil, Kicksdk.endpoints()) ::
          Kicksdk.result(Kicksdk.request())
  def kicks_leaderboard_request(token, top \\ nil, endpoints \\ nil),
    do: Kicksdk.call_request("public_api.kicks_leaderboard_request", %{token: token, top: top}, endpoints)

  @doc ~s(Reads a leaderboard answer: `"lifetime"`, `"month"`, `"week"`, each a list of `"user_id"`, `"username"`, `"rank"`, `"gifted_amount"`.)
  @spec parse_kicks_leaderboard(integer(), binary()) :: Kicksdk.result(map())
  def parse_kicks_leaderboard(status, body),
    do: Kicksdk.call("public_api.parse_kicks_leaderboard", %{status: status, body: body})

  @doc """
  Whether an action worked, for the answers with nothing to read (a deleted
  message, a ban, a channel update, a deleted reward, enrolling in ads,
  updated drops claims): `{:ok, nil}` or `{:error, reason}`.
  """
  @spec parse_done(integer(), binary()) :: Kicksdk.result(nil)
  def parse_done(status, body), do: Kicksdk.call("public_api.parse_done", %{status: status, body: body})
end
