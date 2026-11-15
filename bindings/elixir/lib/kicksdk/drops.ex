defmodule Kicksdk.Drops do
  @moduledoc """
  Drops reward claims (`/public/v1/drops/claims`): an **app** token of the
  organization running the campaigns.
  """

  @filter_keys [:campaign_id, :claim_id, :user_id, :external_status, :limit, :cursor]

  @doc """
  One page of claims, filtered by what is given. Options: `:campaign_id`,
  `:claim_id`, `:user_id`, `:external_status`, `:limit` (1 to 1000),
  `:cursor` (the previous page's `"cursor"`), `:endpoints`.
  """
  @spec claims_request(String.t(), keyword()) :: Kicksdk.result(Kicksdk.request())
  def claims_request(token, opts \\ []),
    do:
      Kicksdk.call_request(
        "drops.claims_request",
        %{token: token, filter: Map.new(@filter_keys, &{&1, opts[&1]})},
        opts[:endpoints]
      )

  @doc ~s(Reads claims: `%{"claims" => […], "cursor" => …}`.)
  @spec parse_claims(integer(), binary()) :: Kicksdk.result(map())
  def parse_claims(status, body), do: Kicksdk.call("drops.parse_claims", %{status: status, body: body})

  @doc """
  Sets claims' `external_status`: `[{claim_id, status}]`, up to 100. Read the
  answer with `Kicksdk.PublicApi.parse_done/2`.
  """
  @spec update_claims_request(String.t(), [{String.t(), String.t()}], Kicksdk.endpoints()) ::
          Kicksdk.result(Kicksdk.request())
  def update_claims_request(token, updates, endpoints \\ nil),
    do:
      Kicksdk.call_request(
        "drops.update_claims_request",
        %{token: token, updates: for({id, status} <- updates, do: %{claim_id: id, external_status: status})},
        endpoints
      )
end
