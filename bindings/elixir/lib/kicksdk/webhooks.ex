defmodule Kicksdk.Webhooks do
  @moduledoc """
  Kick's official webhooks.

  `verify/4` checks a delivery's signature (RSA, SHA-256, PKCS#1 v1.5 over
  `<message id>.<timestamp>.<raw body>`), that it is fresh, and reads it.

  What a valid signature means: Kick sent this body, at that timestamp,
  under that message id, **to some app** - every app shares the key, so
  anyone who receives deliveries can replay one to you. So:

    * the timestamp must be within 5 minutes of now (`:tolerance_secs`);
    * keep the message ids you have seen for at least that long and drop
      repeats (Kick itself sends some events twice);
    * the type, version and subscription-id headers are not signed: the body
      must be the shape of the claimed type, or it is refused.

  Read Kick's key once with `public_key/1` (from
  `Kicksdk.PublicApi.public_key_request/1`) and pass the key to every
  `verify/4`. See `docs/KICK.md` §4.
  """

  @typedoc "Kick's public key, read once by `public_key/1`."
  @type public_key :: reference()

  @typedoc "A delivery's headers: `[{name, value}]` (as Plug's `req_headers`) or a map, names in any case."
  @type headers :: [{String.t(), String.t()}] | %{optional(String.t()) => String.t()}

  @doc """
  Reads Kick's public key (PEM: `BEGIN PUBLIC KEY` or `BEGIN RSA PUBLIC
  KEY`): `{:ok, key}` or `{:error, {:webhook, "unreadable public key"}}`.
  """
  @spec public_key(binary()) :: Kicksdk.result(public_key())
  def public_key(pem) when is_binary(pem), do: Kicksdk.Native.webhook_public_key(pem)

  @doc """
  Verifies and reads a delivery: `{:ok, %{"headers" => headers, "event" =>
  event}}`, `{:error, {:webhook, message}}` when the signature, a header or
  the timestamp is wrong, or `{:error, {:shape, what}}` when a signed body is
  not the shape of its type.

  `key` is from `public_key/1` (or a PEM, read on every call). `body` is the
  raw body, byte for byte, UTF-8 or not: read it before any parser touches
  it.

  Options:

    * `:now` - now, Unix seconds (default `System.os_time(:second)`).
    * `:tolerance_secs` - how far from `:now` the timestamp may be, either
      way (default 300).
    * `:skip_freshness` - `true` to skip the timestamp check, for a delivery
      stored earlier and checked again. A live receiver never should.
  """
  @spec verify(public_key() | binary(), headers(), binary(), keyword()) ::
          Kicksdk.result(%{required(String.t()) => map()})
  def verify(key, headers, body, opts \\ []) when is_binary(body) do
    opts = Keyword.validate!(opts, now: nil, tolerance_secs: 300, skip_freshness: false)
    headers = Enum.to_list(headers)

    if Kicksdk.Native.dirty?(body),
      do: Kicksdk.Native.verify_webhook_dirty(key, headers, body, freshness(opts)),
      else: Kicksdk.Native.verify_webhook(key, headers, body, freshness(opts))
  end

  defp freshness(opts) do
    case {opts[:skip_freshness], opts[:now], opts[:tolerance_secs]} do
      {true, nil, _} ->
        :skip

      {true, _, _} ->
        raise ArgumentError, "pass :now or skip_freshness: true, not both"

      {false, now, tolerance}
      when (is_nil(now) or (is_integer(now) and now >= 0)) and is_integer(tolerance) and
             tolerance >= 0 ->
        {now || System.os_time(:second), tolerance}

      _ ->
        raise ArgumentError,
              ":now and :tolerance_secs must be non-negative integers (seconds), :skip_freshness a boolean"
    end
  end

  @doc """
  Reads a body (raw bytes) by its type, without checking anything: `{:ok,
  event}`, or `{:error, reason}` for a body that is not JSON or not the
  type's shape.
  """
  @spec parse(String.t(), binary()) :: Kicksdk.result(map())
  def parse(event_type, body),
    do:
      if(Kicksdk.Native.dirty?(body),
        do: Kicksdk.Native.parse_webhook_dirty(event_type, body),
        else: Kicksdk.Native.parse_webhook(event_type, body)
      )
end
