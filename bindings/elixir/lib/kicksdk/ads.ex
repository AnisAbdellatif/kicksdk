defmodule Kicksdk.Ads do
  @moduledoc """
  Ads on the token's own channel: a user token with `ads:write` (`ads:read`
  is enough for `status_request/2`). A refusal's reason (e.g.
  `CHANNEL_IS_NOT_LIVE`) is in the `{:status, code, excerpt}` excerpt.
  """

  @doc "Starts an ad break of `seconds` (7 to 90). `id` is a UUID the app makes up for it."
  @spec ad_break_request(String.t(), String.t(), pos_integer(), Kicksdk.endpoints()) ::
          Kicksdk.result(Kicksdk.request())
  def ad_break_request(token, id, seconds, endpoints \\ nil),
    do: Kicksdk.call_request("ads.ad_break_request", %{token: token, id: id, seconds: seconds}, endpoints)

  @doc ~s(Reads an ad break answer: `"ad_break_id"`, `"remaining_ad_breaks"`, ….)
  @spec parse_ad_break(integer(), binary()) :: Kicksdk.result(map())
  def parse_ad_break(status, body), do: Kicksdk.call("ads.parse_ad_break", %{status: status, body: body})

  @doc "The channel's ad break status."
  @spec status_request(String.t(), Kicksdk.endpoints()) :: Kicksdk.result(Kicksdk.request())
  def status_request(token, endpoints \\ nil),
    do: Kicksdk.call_request("ads.status_request", %{token: token}, endpoints)

  @doc ~s(Reads a status answer: `"opted_in"`, `"remaining_ad_breaks"`, `"limits"`, `"ad_breaks"`, ….)
  @spec parse_status(integer(), binary()) :: Kicksdk.result(map())
  def parse_status(status, body), do: Kicksdk.call("ads.parse_status", %{status: status, body: body})

  @doc "Opts the channel in to ads; read the answer with `Kicksdk.PublicApi.parse_done/2`."
  @spec enroll_request(String.t(), Kicksdk.endpoints()) :: Kicksdk.result(Kicksdk.request())
  def enroll_request(token, endpoints \\ nil),
    do: Kicksdk.call_request("ads.enroll_request", %{token: token}, endpoints)
end
