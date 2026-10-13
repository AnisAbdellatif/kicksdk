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

  # Acting as a viewer: `session` is kick.com's `session_token`. Never log it.
  # These go through Kicksdk.call/2 and return `{:error, reason}` for
  # arguments Kick would refuse anyway (a timeout over a week, an empty
  # message).

  @doc "The signed-in viewer's standing on a channel."
  def me_request(session, channel_slug, endpoints \\ nil),
    do: Kicksdk.call_request("website.me_request", %{session: session, channel_slug: channel_slug}, endpoints)

  @doc "Reads a `me` answer: `is_moderator`, `is_broadcaster`, `is_super_admin`."
  def parse_me(status, body), do: Kicksdk.call("website.parse_me", %{status: status, body: body})

  @doc "Bans a user for good."
  def ban_request(session, channel_slug, username, endpoints \\ nil),
    do:
      Kicksdk.call_request(
        "website.ban_request",
        %{session: session, channel_slug: channel_slug, username: username},
        endpoints
      )

  @doc "Times a user out for `minutes` (1 to 10080)."
  def timeout_request(session, channel_slug, username, minutes, endpoints \\ nil),
    do:
      Kicksdk.call_request(
        "website.timeout_request",
        %{session: session, channel_slug: channel_slug, username: username, minutes: minutes},
        endpoints
      )

  @doc "Lifts a ban or timeout."
  def unban_request(session, channel_slug, username, endpoints \\ nil),
    do:
      Kicksdk.call_request(
        "website.unban_request",
        %{session: session, channel_slug: channel_slug, username: username},
        endpoints
      )

  @doc "Deletes a message (needs the chatroom id)."
  def delete_message_request(session, chatroom_id, message_id, endpoints \\ nil),
    do:
      Kicksdk.call_request(
        "website.delete_message_request",
        %{session: session, chatroom_id: chatroom_id, message_id: message_id},
        endpoints
      )

  @doc "Pins `message` (the feed's data for it, whole) for `minutes`."
  def pin_request(session, channel_slug, message, minutes, endpoints \\ nil),
    do:
      Kicksdk.call_request(
        "website.pin_request",
        %{session: session, channel_slug: channel_slug, message: message, minutes: minutes},
        endpoints
      )

  @doc "Removes the pin."
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
  def parse_action(status, body), do: Kicksdk.call("website.parse_action", %{status: status, body: body})
end
