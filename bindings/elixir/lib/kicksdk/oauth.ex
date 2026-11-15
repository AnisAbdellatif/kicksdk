defmodule Kicksdk.OAuth do
  @moduledoc """
  Kick's user OAuth (`id.kick.com`): sign a user in with PKCE, refresh,
  revoke and introspect their token. See `docs/KICK.md` §2.

  The core draws no random numbers: `new_pkce/0` and `new_state/0` draw them
  from `:crypto.strong_rand_bytes/1`.
  """

  @doc """
  A new PKCE pair, `%{"verifier" => …, "challenge" => …}`, from 32 random
  bytes. Keep the verifier until the user comes back.
  """
  @spec new_pkce() :: map()
  def new_pkce do
    {:ok, pkce} = pkce(random())
    pkce
  end

  @doc "A new `state` for `authorize_url/6`: 32 random bytes, base64url without padding."
  @spec new_state() :: String.t()
  def new_state, do: random()

  defp random, do: Base.url_encode64(:crypto.strong_rand_bytes(32), padding: false)

  @doc """
  The PKCE pair for `random`: 32 to 96 bytes from a CSPRNG, as base64url
  without padding. `new_pkce/0` draws them.
  """
  @spec pkce(String.t()) :: Kicksdk.result(map())
  def pkce(random), do: Kicksdk.call("oauth.pkce", %{random: random})

  @doc "The PKCE pair for a verifier made elsewhere (or kept from before)."
  @spec pkce_from_verifier(String.t()) :: Kicksdk.result(map())
  def pkce_from_verifier(verifier), do: Kicksdk.call("oauth.pkce_from_verifier", %{verifier: verifier})

  @doc """
  Where to send the user's browser to approve the app. `redirect_uri` is
  exactly the one in the app's settings; `state` is `new_state/0`'s; the
  challenge is `verifier`'s.
  """
  @spec authorize_url(String.t(), String.t(), [String.t()], String.t(), String.t(), Kicksdk.endpoints()) ::
          Kicksdk.result(String.t())
  def authorize_url(client_id, redirect_uri, scopes, state, verifier, endpoints \\ nil),
    do:
      Kicksdk.call_request(
        "oauth.authorize_url",
        %{client_id: client_id, redirect_uri: redirect_uri, scopes: scopes, state: state, verifier: verifier},
        endpoints
      )

  @doc "A user token for the `code` on the redirect, with the PKCE verifier and the same `redirect_uri`."
  @spec code_request(String.t(), String.t(), String.t(), String.t(), String.t(), Kicksdk.endpoints()) ::
          Kicksdk.result(Kicksdk.request())
  def code_request(client_id, client_secret, redirect_uri, code, code_verifier, endpoints \\ nil),
    do:
      Kicksdk.call_request(
        "oauth.code_request",
        %{
          client_id: client_id,
          client_secret: client_secret,
          redirect_uri: redirect_uri,
          code: code,
          code_verifier: code_verifier
        },
        endpoints
      )

  @doc "A new user token (and refresh token) for a refresh token."
  @spec refresh_request(String.t(), String.t(), String.t(), Kicksdk.endpoints()) ::
          Kicksdk.result(Kicksdk.request())
  def refresh_request(client_id, client_secret, refresh_token, endpoints \\ nil),
    do:
      Kicksdk.call_request(
        "oauth.refresh_request",
        %{client_id: client_id, client_secret: client_secret, refresh_token: refresh_token},
        endpoints
      )

  @doc ~s(Reads a token answer: `"access_token"`, `"refresh_token"`, `"token_type"`, `"expires_in"`, `"scopes"`.)
  @spec parse_user_token(integer(), binary()) :: Kicksdk.result(map())
  def parse_user_token(status, body),
    do: Kicksdk.call("oauth.parse_user_token", %{status: status, body: body})

  @doc "Revokes an access or refresh token; `token_type_hint` is `\"access_token\"` or `\"refresh_token\"`."
  @spec revoke_request(String.t(), String.t() | nil, Kicksdk.endpoints()) :: Kicksdk.result(Kicksdk.request())
  def revoke_request(token, token_type_hint \\ nil, endpoints \\ nil),
    do:
      Kicksdk.call_request(
        "oauth.revoke_request",
        %{token: token, token_type_hint: token_type_hint},
        endpoints
      )

  @doc "Whether the token was revoked: `{:ok, nil}` or `{:error, reason}`."
  @spec parse_revoke(integer(), binary()) :: Kicksdk.result(nil)
  def parse_revoke(status, body), do: Kicksdk.call("oauth.parse_revoke", %{status: status, body: body})

  @doc "Whether a token is active, and what it may do."
  @spec introspect_request(String.t(), Kicksdk.endpoints()) :: Kicksdk.result(Kicksdk.request())
  def introspect_request(token, endpoints \\ nil),
    do: Kicksdk.call_request("oauth.introspect_request", %{token: token}, endpoints)

  @doc ~s(Reads an introspection answer: `"active"`, `"client_id"`, `"exp"`, `"scopes"`, `"token_type"`.)
  @spec parse_introspect(integer(), binary()) :: Kicksdk.result(map())
  def parse_introspect(status, body),
    do: Kicksdk.call("oauth.parse_introspect", %{status: status, body: body})

  @doc "The scopes Kick documents, e.g. `\"chat:write\"`."
  @spec scopes() :: [String.t()]
  def scopes do
    {:ok, scopes} = Kicksdk.call("oauth.scopes", %{})
    scopes
  end
end
