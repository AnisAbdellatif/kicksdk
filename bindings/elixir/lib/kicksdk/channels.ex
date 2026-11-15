defmodule Kicksdk.Channels do
  @moduledoc """
  The token's own channel (`PATCH /public/v1/channels`): a user token with
  `channel:write`.
  """

  @doc """
  Sets what is given of the stream's category and title and its custom tags
  (`[]` clears them; at most 10); the rest stays. Options: `:category_id`,
  `:custom_tags`, `:stream_title`, `:endpoints`. Read the answer with
  `Kicksdk.PublicApi.parse_done/2`.
  """
  @spec update_request(String.t(), keyword()) :: Kicksdk.result(Kicksdk.request())
  def update_request(token, opts \\ []) do
    args = %{
      token: token,
      category_id: opts[:category_id],
      custom_tags: opts[:custom_tags],
      stream_title: opts[:stream_title]
    }

    Kicksdk.call_request("channels.update_request", args, opts[:endpoints])
  end
end
