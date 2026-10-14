defmodule Kicksdk.Website do
  @moduledoc """
  The endpoints kick.com's own frontend uses (`kick.com/api/v2`).

  Undocumented and a grey area under Kick's terms: treat a failure as a gap,
  never a zero. The only source of the **chatroom id** and the **follower
  total**. From a server, a bare `User-Agent: Mozilla/5.0` passes where a full
  browser one is refused ("Request blocked by security policy").

  A slug or username that is empty, `.` or `..`, or not UTF-8, is refused:
  those builders return `{:ok, request}` or `{:error, {:invalid, what}}`.
  """

  import Kicksdk, only: [encode_endpoints: 1, request: 1]
  alias Kicksdk.Native

  @doc "A channel by slug."
  @spec channel_request(String.t(), Kicksdk.endpoints()) :: Kicksdk.result(Kicksdk.request())
  def channel_request(slug, endpoints \\ nil),
    do: request(Native.channel_request(encode_endpoints(endpoints), slug))

  @doc "Reads a channel answer: chatroom id, followers (number or string upstream), live state."
  @spec parse_channel(integer(), binary()) :: Kicksdk.result(map())
  def parse_channel(status, body),
    do:
      if(Native.dirty?(body),
        do: Native.parse_channel_dirty(status, body),
        else: Native.parse_channel(status, body)
      )

  @doc "Recent chat and the pin, by channel id."
  @spec history_request(non_neg_integer(), Kicksdk.endpoints()) :: Kicksdk.request()
  def history_request(channel_id, endpoints \\ nil),
    do: request(Native.history_request(encode_endpoints(endpoints), channel_id))

  @doc ~s(Reads a history answer: `%{"messages" => […], "pinned" => event | nil}`.)
  @spec parse_history(integer(), binary()) :: Kicksdk.result(map())
  def parse_history(status, body),
    do:
      if(Native.dirty?(body),
        do: Native.parse_history_dirty(status, body),
        else: Native.parse_history(status, body)
      )

  @doc "A user as a channel sees them; a 404 means the channel does not know the name."
  @spec channel_user_request(String.t(), String.t(), Kicksdk.endpoints()) :: Kicksdk.result(Kicksdk.request())
  def channel_user_request(slug, username, endpoints \\ nil),
    do: request(Native.channel_user_request(encode_endpoints(endpoints), slug, username))

  @doc "Reads a channel user answer."
  @spec parse_channel_user(integer(), binary()) :: Kicksdk.result(map())
  def parse_channel_user(status, body),
    do:
      if(Native.dirty?(body),
        do: Native.parse_channel_user_dirty(status, body),
        else: Native.parse_channel_user(status, body)
      )

  # Acting as a viewer: `session` is kick.com's `session_token`. Never log it.
  # These go through Kicksdk.call/2 and, like every builder that can refuse
  # its arguments, return `{:ok, request}` or `{:error, reason}`: for what
  # Kick would refuse anyway (a timeout over a week, an empty message), a
  # path segment that is empty, `.` or `..`, a session token with a line
  # break.

  @doc "The signed-in viewer's standing on a channel."
  @spec me_request(String.t(), String.t(), Kicksdk.endpoints()) :: Kicksdk.result(Kicksdk.request())
  def me_request(session, channel_slug, endpoints \\ nil),
    do: Kicksdk.call_request("website.me_request", %{session: session, channel_slug: channel_slug}, endpoints)

  @doc "Reads a `me` answer: `is_moderator`, `is_broadcaster`, `is_super_admin`."
  @spec parse_me(integer(), binary()) :: Kicksdk.result(map())
  def parse_me(status, body), do: Kicksdk.call("website.parse_me", %{status: status, body: body})

  @doc "Bans a user for good."
  @spec ban_request(String.t(), String.t(), String.t(), Kicksdk.endpoints()) ::
          Kicksdk.result(Kicksdk.request())
  def ban_request(session, channel_slug, username, endpoints \\ nil),
    do:
      Kicksdk.call_request(
        "website.ban_request",
        %{session: session, channel_slug: channel_slug, username: username},
        endpoints
      )

  @doc "Times a user out for `minutes` (1 to 10080)."
  @spec timeout_request(String.t(), String.t(), String.t(), pos_integer(), Kicksdk.endpoints()) ::
          Kicksdk.result(Kicksdk.request())
  def timeout_request(session, channel_slug, username, minutes, endpoints \\ nil),
    do:
      Kicksdk.call_request(
        "website.timeout_request",
        %{session: session, channel_slug: channel_slug, username: username, minutes: minutes},
        endpoints
      )

  @doc "Lifts a ban or timeout."
  @spec unban_request(String.t(), String.t(), String.t(), Kicksdk.endpoints()) ::
          Kicksdk.result(Kicksdk.request())
  def unban_request(session, channel_slug, username, endpoints \\ nil),
    do:
      Kicksdk.call_request(
        "website.unban_request",
        %{session: session, channel_slug: channel_slug, username: username},
        endpoints
      )

  @doc "Deletes a message (needs the chatroom id)."
  @spec delete_message_request(String.t(), non_neg_integer(), String.t(), Kicksdk.endpoints()) ::
          Kicksdk.result(Kicksdk.request())
  def delete_message_request(session, chatroom_id, message_id, endpoints \\ nil),
    do:
      Kicksdk.call_request(
        "website.delete_message_request",
        %{session: session, chatroom_id: chatroom_id, message_id: message_id},
        endpoints
      )

  @doc "Pins `message` (the feed's data for it, whole) for `minutes`."
  @spec pin_request(String.t(), String.t(), map() | String.t(), pos_integer(), Kicksdk.endpoints()) ::
          Kicksdk.result(Kicksdk.request())
  def pin_request(session, channel_slug, message, minutes, endpoints \\ nil),
    do:
      Kicksdk.call_request(
        "website.pin_request",
        %{session: session, channel_slug: channel_slug, message: message, minutes: minutes},
        endpoints
      )

  @doc "Removes the pin."
  @spec unpin_request(String.t(), String.t(), Kicksdk.endpoints()) :: Kicksdk.result(Kicksdk.request())
  def unpin_request(session, channel_slug, endpoints \\ nil),
    do:
      Kicksdk.call_request(
        "website.unpin_request",
        %{session: session, channel_slug: channel_slug},
        endpoints
      )

  @doc """
  Sends a message, or a reply when `reply_to` is
  `%{message_id: …, content: …, sender_id: …, sender_username: …}`.
  """
  @spec send_request(
          String.t(),
          non_neg_integer(),
          String.t(),
          String.t(),
          map() | nil,
          Kicksdk.endpoints()
        ) :: Kicksdk.result(Kicksdk.request())
  def send_request(session, chatroom_id, content, message_ref, reply_to \\ nil, endpoints \\ nil),
    do:
      Kicksdk.call_request(
        "website.send_request",
        %{
          session: session,
          chatroom_id: chatroom_id,
          content: content,
          message_ref: message_ref,
          reply_to: reply_to
        },
        endpoints
      )

  @doc "Whether an acting call worked: `{:ok, nil}` or `{:error, {:status, code, excerpt}}`."
  @spec parse_action(integer(), binary()) :: Kicksdk.result(nil)
  def parse_action(status, body), do: Kicksdk.call("website.parse_action", %{status: status, body: body})
end
