defmodule Kicksdk.Webhooks do
  @moduledoc """
  Kick's official webhooks.

  `verify/3` checks a delivery's signature (RSA, SHA-256, PKCS#1 v1.5 over
  `<message id>.<timestamp>.<raw body>`) and reads it. The type, version and
  subscription-id headers are not signed. The same event can arrive twice: key
  on the message id. See `docs/KICK.md` §4.
  """

  @doc """
  Verifies and reads a delivery. `headers` as `[{name, value}]` (any case),
  `body` the raw body. `{:ok, %{"headers" => headers, "event" => event}}` or
  `{:error, reason}`.
  """
  @spec verify(String.t(), [{String.t(), String.t()}], String.t()) :: {:ok, map()} | {:error, String.t()}
  def verify(public_key_pem, headers, body), do: Kicksdk.Native.verify_webhook(public_key_pem, headers, body)

  @doc "Reads a body by its type, without checking anything."
  @spec parse(String.t(), String.t()) :: {:ok, map()} | {:error, term()}
  def parse(event_type, body), do: Kicksdk.Native.parse_webhook(event_type, body)
end
