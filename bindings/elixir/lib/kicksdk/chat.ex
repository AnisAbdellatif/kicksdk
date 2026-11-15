defmodule Kicksdk.Chat do
  @moduledoc """
  Chat as a user or the app's bot (`/public/v1/chat`): a user token with
  `chat:write`; deleting needs `moderation:chat_message:manage`.
  """

  @typedoc "Who a message goes out as: the token's user into a broadcaster's chat, or the app's bot."
  @type sender :: {:user, non_neg_integer()} | :bot

  @doc """
  Sends `content` as `{:user, broadcaster_user_id}` (the token's user, into
  that channel's chat) or `:bot` (the app's bot, into the token's own
  channel). Options: `:reply_to_message_id`, `:endpoints`.
  """
  @spec send_request(String.t(), sender(), String.t(), keyword()) :: Kicksdk.result(Kicksdk.request())
  def send_request(token, sender, content, opts \\ []) do
    args = %{
      token: token,
      sender: encode_sender(sender),
      content: content,
      reply_to_message_id: opts[:reply_to_message_id]
    }

    Kicksdk.call_request("chat.send_request", args, opts[:endpoints])
  end

  defp encode_sender({:user, broadcaster_user_id}), do: %{user: broadcaster_user_id}
  defp encode_sender(:bot), do: "bot"

  @doc ~s(Reads a send answer: `%{"is_sent" => …, "message_id" => …}`.)
  @spec parse_send(integer(), binary()) :: Kicksdk.result(map())
  def parse_send(status, body), do: Kicksdk.call("chat.parse_send", %{status: status, body: body})

  @doc "Deletes a message by id; read the answer with `Kicksdk.PublicApi.parse_done/2`."
  @spec delete_request(String.t(), String.t(), Kicksdk.endpoints()) :: Kicksdk.result(Kicksdk.request())
  def delete_request(token, message_id, endpoints \\ nil),
    do: Kicksdk.call_request("chat.delete_request", %{token: token, message_id: message_id}, endpoints)
end
