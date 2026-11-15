defmodule Kicksdk.UserApiTest do
  # The user OAuth flow and the user-token (and drops) endpoints against
  # fixtures/docs/: each `actions-*` file holds the request Kick documents and
  # an answer, as crates/kicksdk/tests/public_api_actions.rs reads them.
  use ExUnit.Case, async: true

  alias Kicksdk.{Ads, Channels, Chat, Drops, Moderation, OAuth, PublicApi, Rewards}

  @docs Path.expand("../../../fixtures/docs", __DIR__)
  @reward_id "01HZ8X9K2M4N6P8Q0R2S4T6V8W0Y2Z4"
  @message_id "828f83bb-e391-4c78-9e91-af35a172840e"

  defp fixture(name), do: Path.join([@docs, "public_api", name <> ".json"]) |> File.read!() |> JSON.decode!()
  defp answer(%{"response" => %{"status" => status, "body" => body}}), do: {status, body}
  defp oauth(name), do: File.read!(Path.join([@docs, "oauth", name]))

  # The request is the documented one: method, URL, query, JSON body, token.
  defp same({:ok, r}, %{"request" => want}) do
    uri = URI.parse(r["url"])
    assert r["method"] == want["method"]
    assert "#{uri.scheme}://#{uri.host}#{uri.path}" == want["url"]
    assert URI.decode_query(uri.query || "") == Map.new(want["params"], fn {k, v} -> {k, to_string(v)} end)
    assert if(r["body"], do: JSON.decode!(r["body"])) == want["json"]
    assert {"authorization", "Bearer t"} in r["headers"]
  end

  defp done(f) do
    {s, b} = answer(f)
    assert {:ok, nil} = PublicApi.parse_done(s, b)
  end

  describe "OAuth" do
    test "PKCE and state are drawn fresh, 43 characters of base64url" do
      %{"verifier" => v, "challenge" => c} = a = OAuth.new_pkce()
      assert byte_size(v) == 43
      refute OAuth.new_pkce()["verifier"] == v
      assert c == Base.url_encode64(:crypto.hash(:sha256, v), padding: false)
      assert {:ok, ^a} = OAuth.pkce_from_verifier(v)
      assert OAuth.new_state() =~ ~r/^[A-Za-z0-9_-]{43}$/
      assert {:error, {:invalid, _}} = OAuth.pkce(Base.url_encode64(<<0::128>>, padding: false))
      assert {:error, {:invalid, _}} = OAuth.pkce_from_verifier("short")
    end

    test "the authorize URL carries the scopes, state and challenge" do
      %{"verifier" => v, "challenge" => c} = OAuth.new_pkce()
      {:ok, url} = OAuth.authorize_url("cid", "http://localhost/cb", ["user:read", "chat:write"], "s1", v)
      uri = URI.parse(url)
      assert uri.host == "id.kick.com"

      assert %{"scope" => "user:read chat:write", "code_challenge" => ^c, "state" => "s1"} =
               URI.decode_query(uri.query)

      assert {:error, {:invalid, _}} = OAuth.authorize_url("cid", "http://localhost/cb", [], "s1", v)
      assert length(OAuth.scopes()) == 13
      assert "chat:write" in OAuth.scopes()
    end

    test "token, revoke and introspect requests and answers" do
      assert {:ok, %{"method" => "POST", "url" => "https://id.kick.com/oauth/token", "body" => body}} =
               OAuth.code_request("cid", "secret", "http://localhost/cb", "c0de", String.duplicate("v", 43))

      assert %{"grant_type" => "authorization_code"} = URI.decode_query(body)

      assert {:ok, %{"url" => "http://x/oauth/token", "body" => body}} =
               OAuth.refresh_request("cid", "secret", "r", %{id: "http://x"})

      assert %{"refresh_token" => "r"} = URI.decode_query(body)
      assert {:ok, %{"expires_in" => 7200}} = OAuth.parse_user_token(200, oauth("token-spec.json"))
      assert {:error, {:status, 400, _}} = OAuth.parse_user_token(400, oauth("error.json"))

      assert {:ok, %{"url" => url}} = OAuth.revoke_request("t", "refresh_token")
      assert url =~ "token_type_hint=refresh_token"
      assert {:error, {:call, _}} = OAuth.revoke_request("t", "id_token")
      assert {:ok, nil} = OAuth.parse_revoke(200, oauth("revoke.txt"))

      assert {:ok, %{"method" => "POST"}} = OAuth.introspect_request("t")

      assert {:ok, %{"active" => true, "scopes" => ["user:read", "channel:read"]}} =
               OAuth.parse_introspect(200, oauth("introspect.json"))
    end
  end

  test "PublicApi: users, categories, stats, Kicks leaderboard" do
    assert {:ok, %{"url" => "https://api.kick.com/public/v1/users?id=123"}} =
             PublicApi.users_request("t", [123])

    assert {:ok, %{"url" => "https://api.kick.com/public/v1/users"}} = PublicApi.users_request("t")
    {s, b} = answer(fixture("v1-users"))
    assert {:ok, [%{"email" => "john.doe@example.com"}]} = PublicApi.parse_users(s, b)

    {:ok, r} = PublicApi.categories_request("t", names: ["Rust"], limit: 5)
    assert URI.decode_query(URI.parse(r["url"]).query) == %{"name" => "Rust", "limit" => "5"}
    {s, b} = answer(fixture("v2-categories"))
    assert {:ok, %{"data" => [%{"name" => "Rust"}], "next_cursor" => c}} = PublicApi.parse_categories(s, b)
    assert c

    assert {:ok, %{"url" => "https://api.kick.com/public/v1/livestreams/stats"}} =
             PublicApi.livestream_stats_request("t")

    {s, b} = answer(fixture("v1-livestreams-stats"))
    assert {:ok, %{"total_count" => 100}} = PublicApi.parse_livestream_stats(s, b)

    assert {:ok, %{"url" => "https://api.kick.com/public/v1/kicks/leaderboard?top=1"}} =
             PublicApi.kicks_leaderboard_request("t", 1)

    assert {:error, {:invalid, _}} = PublicApi.kicks_leaderboard_request("t", 101)
    {s, b} = answer(fixture("v1-kicks-leaderboard"))
    assert {:ok, %{"week" => [%{"username" => "john-doe"}]}} = PublicApi.parse_kicks_leaderboard(s, b)

    assert {:ok, nil} = PublicApi.parse_done(204, "")
    assert {:error, {:status, 401, _}} = PublicApi.parse_done(401, "{}")
  end

  test "Chat" do
    f = fixture("actions-chat-send")
    same(Chat.send_request("t", {:user, 123}, "Pog", reply_to_message_id: @message_id), f)
    {s, b} = answer(f)
    assert {:ok, %{"is_sent" => true, "message_id" => @message_id}} = Chat.parse_send(s, b)
    {:ok, bot} = Chat.send_request("t", :bot, "hi")
    assert JSON.decode!(bot["body"]) == %{"content" => "hi", "type" => "bot"}
    assert {:error, {:invalid, _}} = Chat.send_request("t", :bot, "")

    f = fixture("actions-chat-delete")
    same(Chat.delete_request("t", @message_id), f)
    done(f)
  end

  test "Moderation" do
    f = fixture("actions-moderation-timeout")
    same(Moderation.timeout_request("t", 123, 333, 10, reason: "Not vibing"), f)
    done(f)
    same(Moderation.unban_request("t", 123, 333), fixture("actions-moderation-unban"))
    {:ok, ban} = Moderation.ban_request("t", 123, 333)
    assert JSON.decode!(ban["body"]) == %{"broadcaster_user_id" => 123, "user_id" => 333}
    assert {:error, {:invalid, _}} = Moderation.timeout_request("t", 123, 333, 20_000)
  end

  test "Channels" do
    f = fixture("actions-channels-update")

    same(
      Channels.update_request("t",
        category_id: 123,
        custom_tags: ["pog", "chad"],
        stream_title: "My first stream"
      ),
      f
    )

    done(f)
    {:ok, r} = Channels.update_request("t", custom_tags: [])
    assert JSON.decode!(r["body"]) == %{"custom_tags" => []}
    assert {:error, {:invalid, _}} = Channels.update_request("t")
  end

  test "Rewards" do
    f = fixture("actions-rewards-list")
    same(Rewards.list_request("t"), f)
    {s, b} = answer(f)
    assert {:ok, [%{"id" => @reward_id}]} = Rewards.parse_rewards(s, b)

    for {name, update?} <- [{"actions-rewards-create", false}, {"actions-rewards-update", true}] do
      f = fixture(name)
      fields = f["request"]["json"]

      same(
        if(update?,
          do: Rewards.update_request("t", @reward_id, fields),
          else: Rewards.create_request("t", fields)
        ),
        f
      )

      {s, b} = answer(f)
      assert {:ok, %{"id" => @reward_id, "title" => "Song Request"}} = Rewards.parse_reward(s, b)
    end

    assert {:error, {:call, _}} = Rewards.create_request("t", %{titel: "x"})

    f = fixture("actions-rewards-delete")
    same(Rewards.delete_request("t", @reward_id), f)
    done(f)

    f = fixture("actions-rewards-redemptions")
    same(Rewards.redemptions_request("t", status: "pending"), f)
    {s, b} = answer(f)

    assert {:ok,
            %{"rewards" => [%{"reward" => %{"title" => "Unban Request"}, "redemptions" => [x | _]} | _]} =
              page} =
             Rewards.parse_redemptions(s, b)

    assert x["user_input"] == "sorry mr strimer"
    assert page["next_cursor"]

    assert {:ok, %{"url" => url}} = Rewards.redemptions_by_ids_request("t", ["a", "b"], cursor: "c")
    assert URI.parse(url).query == "id=a&id=b&cursor=c"

    ids = ["01KEWZ2RKYZZFGD154K50Y3F8D", "01KEWZ2YC27070XWMHKFJR6C87"]

    for {name, r} <- [
          {"actions-rewards-accept", Rewards.accept_request("t", ids)},
          {"actions-rewards-reject", Rewards.reject_request("t", ids)}
        ] do
      f = fixture(name)
      same(r, f)
      {s, b} = answer(f)

      assert {:ok, [%{"id" => "01KERB68MZZ04551D6X5TNRFYV", "reason" => "NOT_OWNED"} | _]} =
               Rewards.parse_failures(s, b)
    end
  end

  test "Ads" do
    uuid = "123e4567-e89b-12d3-a456-426614174000"
    f = fixture("actions-ads-ad-break")
    same(Ads.ad_break_request("t", uuid, 30), f)
    {s, b} = answer(f)
    assert {:ok, %{"ad_break_id" => "Xq3LmP9aB2cD", "remaining_ad_breaks" => 2}} = Ads.parse_ad_break(s, b)
    {s, b} = answer(fixture("actions-ads-ad-break-refused"))
    assert {:error, {:status, 400, _}} = Ads.parse_ad_break(s, b)

    f = fixture("actions-ads-status")
    same(Ads.status_request("t"), f)
    {s, b} = answer(f)
    assert {:ok, %{"opted_in" => true, "remaining_ad_breaks" => 2}} = Ads.parse_status(s, b)

    f = fixture("actions-ads-enroll")
    same(Ads.enroll_request("t"), f)
    done(f)
  end

  test "Drops" do
    f = fixture("actions-drops-claims")

    same(
      Drops.claims_request("t",
        campaign_id: "01JAXK8N4QWRTY5PM7ZEBVJDS8S",
        user_id: 1234,
        limit: 10,
        cursor: "01K0TDDR08Q5ZNWXK92VRXD7GW"
      ),
      f
    )

    {s, b} = answer(f)
    assert {:ok, %{"claims" => [_], "cursor" => "01K0TDDR08Q5ZNWXK92SDH7SDH"}} = Drops.parse_claims(s, b)
    assert {:ok, %{"url" => "https://api.kick.com/public/v1/drops/claims"}} = Drops.claims_request("t")

    f = fixture("actions-drops-update")

    same(
      Drops.update_claims_request("t", [
        {"01KAAFHJ2PNXS48NG8XXPGWKCZ", "processed"},
        {"01KAAFHJ2PNXS48NG8XXPGWKCA", "initiated"}
      ]),
      f
    )

    done(f)
    {s, b} = answer(fixture("actions-drops-unauthorized"))
    assert {:error, {:status, 401, _}} = Drops.parse_claims(s, b)
  end
end
