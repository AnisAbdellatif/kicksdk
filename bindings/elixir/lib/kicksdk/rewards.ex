defmodule Kicksdk.Rewards do
  @moduledoc """
  Channel point rewards and their redemptions, on the token's own channel: a
  user token with `channel:rewards:read` (reading) or `channel:rewards:write`.
  Ids are ULIDs; only the app that made a reward may change it or settle its
  redemptions.
  """

  @doc "The channel's rewards."
  @spec list_request(String.t(), Kicksdk.endpoints()) :: Kicksdk.result(Kicksdk.request())
  def list_request(token, endpoints \\ nil),
    do: Kicksdk.call_request("rewards.list_request", %{token: token}, endpoints)

  @doc "Reads a `list_request/2` answer."
  @spec parse_rewards(integer(), binary()) :: Kicksdk.result([map()])
  def parse_rewards(status, body), do: Kicksdk.call("rewards.parse_rewards", %{status: status, body: body})

  @doc """
  A new reward. `fields`: `title` and `cost` (needed), `description`,
  `background_color`, `is_enabled`, `is_user_input_required`,
  `should_redemptions_skip_request_queue`.
  """
  @spec create_request(String.t(), map(), Kicksdk.endpoints()) :: Kicksdk.result(Kicksdk.request())
  def create_request(token, fields, endpoints \\ nil),
    do: Kicksdk.call_request("rewards.create_request", %{token: token, fields: fields}, endpoints)

  @doc "Changes the `fields` given (as for `create_request/3`, and `is_paused`)."
  @spec update_request(String.t(), String.t(), map(), Kicksdk.endpoints()) ::
          Kicksdk.result(Kicksdk.request())
  def update_request(token, id, fields, endpoints \\ nil),
    do: Kicksdk.call_request("rewards.update_request", %{token: token, id: id, fields: fields}, endpoints)

  @doc "Reads a reward, from `create_request/3` or `update_request/4`."
  @spec parse_reward(integer(), binary()) :: Kicksdk.result(map())
  def parse_reward(status, body), do: Kicksdk.call("rewards.parse_reward", %{status: status, body: body})

  @doc "Deletes a reward; read the answer with `Kicksdk.PublicApi.parse_done/2`."
  @spec delete_request(String.t(), String.t(), Kicksdk.endpoints()) :: Kicksdk.result(Kicksdk.request())
  def delete_request(token, id, endpoints \\ nil),
    do: Kicksdk.call_request("rewards.delete_request", %{token: token, id: id}, endpoints)

  @doc """
  Redemptions, grouped by reward. Options: `:reward_id` (one reward; none:
  all), `:status` (`"pending"`, Kick's default; `"accepted"`; `"rejected"`),
  `:cursor` (a page's `"next_cursor"`), `:endpoints`.
  """
  @spec redemptions_request(String.t(), keyword()) :: Kicksdk.result(Kicksdk.request())
  def redemptions_request(token, opts \\ []) do
    args = %{token: token, reward_id: opts[:reward_id], status: opts[:status], cursor: opts[:cursor]}
    Kicksdk.call_request("rewards.redemptions_request", args, opts[:endpoints])
  end

  @doc "Redemptions by id, whatever their status. Options: `:cursor`, `:endpoints`."
  @spec redemptions_by_ids_request(String.t(), [String.t()], keyword()) :: Kicksdk.result(Kicksdk.request())
  def redemptions_by_ids_request(token, ids, opts \\ []),
    do:
      Kicksdk.call_request(
        "rewards.redemptions_by_ids_request",
        %{token: token, ids: ids, cursor: opts[:cursor]},
        opts[:endpoints]
      )

  @doc ~s(Reads redemptions: `%{"rewards" => [%{"reward" => …, "redemptions" => […]}], "next_cursor" => …}`.)
  @spec parse_redemptions(integer(), binary()) :: Kicksdk.result(map())
  def parse_redemptions(status, body),
    do: Kicksdk.call("rewards.parse_redemptions", %{status: status, body: body})

  @doc "Accepts up to 25 redemptions; read the answer with `parse_failures/2`."
  @spec accept_request(String.t(), [String.t()], Kicksdk.endpoints()) :: Kicksdk.result(Kicksdk.request())
  def accept_request(token, ids, endpoints \\ nil),
    do: Kicksdk.call_request("rewards.accept_request", %{token: token, ids: ids}, endpoints)

  @doc "Rejects up to 25 redemptions (refunding them); read the answer with `parse_failures/2`."
  @spec reject_request(String.t(), [String.t()], Kicksdk.endpoints()) :: Kicksdk.result(Kicksdk.request())
  def reject_request(token, ids, endpoints \\ nil),
    do: Kicksdk.call_request("rewards.reject_request", %{token: token, ids: ids}, endpoints)

  @doc ~s(The redemptions that were not accepted or rejected: `[%{"id" => …, "reason" => …}]`.)
  @spec parse_failures(integer(), binary()) :: Kicksdk.result([map()])
  def parse_failures(status, body), do: Kicksdk.call("rewards.parse_failures", %{status: status, body: body})
end
