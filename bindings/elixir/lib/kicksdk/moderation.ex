defmodule Kicksdk.Moderation do
  @moduledoc """
  Bans and timeouts (`/public/v1/moderation/bans`): a user token with
  `moderation:ban`. Read the answers with `Kicksdk.PublicApi.parse_done/2`.
  """

  @doc "Bans `user_id` from `broadcaster_user_id`'s chat for good. Options: `:reason`, `:endpoints`."
  @spec ban_request(String.t(), non_neg_integer(), non_neg_integer(), keyword()) ::
          Kicksdk.result(Kicksdk.request())
  def ban_request(token, broadcaster_user_id, user_id, opts \\ []),
    do:
      Kicksdk.call_request(
        "moderation.ban_request",
        %{token: token, broadcaster_user_id: broadcaster_user_id, user_id: user_id, reason: opts[:reason]},
        opts[:endpoints]
      )

  @doc "Times `user_id` out for `minutes` (1 to 10080, a week). Options: `:reason`, `:endpoints`."
  @spec timeout_request(String.t(), non_neg_integer(), non_neg_integer(), pos_integer(), keyword()) ::
          Kicksdk.result(Kicksdk.request())
  def timeout_request(token, broadcaster_user_id, user_id, minutes, opts \\ []),
    do:
      Kicksdk.call_request(
        "moderation.timeout_request",
        %{
          token: token,
          broadcaster_user_id: broadcaster_user_id,
          user_id: user_id,
          minutes: minutes,
          reason: opts[:reason]
        },
        opts[:endpoints]
      )

  @doc "Lifts a ban or a timeout."
  @spec unban_request(String.t(), non_neg_integer(), non_neg_integer(), Kicksdk.endpoints()) ::
          Kicksdk.result(Kicksdk.request())
  def unban_request(token, broadcaster_user_id, user_id, endpoints \\ nil),
    do:
      Kicksdk.call_request(
        "moderation.unban_request",
        %{token: token, broadcaster_user_id: broadcaster_user_id, user_id: user_id},
        endpoints
      )
end
