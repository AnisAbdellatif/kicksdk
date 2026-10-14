defmodule Kicksdk.ConventionsTest do
  # What the binding returns and raises, whatever the input: errors as values
  # where the data can be wrong, raised where no call could work.
  use ExUnit.Case, async: true

  alias Kicksdk.{Events, PublicApi, Realtime, Website}
  alias Kicksdk.Realtime.Session

  @not_utf8 <<"{\"data\":\"", 255, "\"}">>

  describe "text that is not UTF-8" do
    test "fails a parser as a value, keeping Kick's error status" do
      assert {:error, {:json, "the body is not UTF-8"}} = Website.parse_channel(200, @not_utf8)
      assert {:error, {:json, _}} = Realtime.parse_token(200, @not_utf8)
      assert {:error, {:json, _}} = PublicApi.parse_livestreams(200, @not_utf8)
      assert {:error, {:status, 503, _}} = Website.parse_history(503, @not_utf8)
    end

    test "is refused by a builder that can refuse" do
      assert {:error, {:invalid, "the slug is not UTF-8"}} = Website.channel_request(<<255>>)
      assert {:error, {:invalid, _}} = Website.channel_user_request("somechannel", <<255>>)
      assert {:error, {:invalid, _}} = PublicApi.channels_by_slugs_request("t", ["somechannel", <<255>>])
      assert {:error, {:invalid, _}} = PublicApi.subscriptions_request(<<255>>)
      assert {:error, {:invalid, _}} = PublicApi.unsubscribe_requests("t", [<<255>>])
      assert {:error, {:invalid, _}} = PublicApi.subscribe_request("t", 1, [{<<255>>, 1}])
      assert {:error, {:invalid, _}} = Events.emote_url(<<255>>)
    end

    test "raises a clear ArgumentError where a plain value is returned" do
      assert_raise ArgumentError, "invalid request: the message is not UTF-8", fn ->
        Events.parse_content(<<255>>)
      end

      assert_raise ArgumentError, ~r/client id is not UTF-8/, fn -> Realtime.token_request(<<255>>) end
      assert_raise ArgumentError, ~r/token is not UTF-8/, fn -> Session.open(Session.new([]), <<255>>, 0) end
    end

    test "is read as text by the lenient event parser" do
      assert %{"type" => _} = Events.parse("App\\Events\\ChatMessageEvent", @not_utf8)
    end

    test "skips a frame's lines like lines that are not JSON" do
      s = Session.new(["chatrooms.1.v2"])
      Session.open(s, "t", 0)
      frame = <<255, "\n">> <> ~s({"id":1,"connect":{"client":"c","ping":25}})
      assert [%{"action" => "connected"} | _] = Session.receive(s, frame, 1)
      assert Session.connected?(s)
    end

    test "comes back from call/2 as an error, not a raise" do
      assert {:error, {:call, "the arguments do not encode as JSON: a string is not UTF-8" <> _}} =
               Kicksdk.call("website.channel_request", %{slug: <<255>>})

      assert {:error, {:call, _}} = Kicksdk.call("website.channel_request", %{slug: {:not, :json}})
      assert {:error, {:call, "the function name is not UTF-8"}} = Kicksdk.call(<<255>>, %{})
    end
  end

  describe "statuses" do
    test "out of range are refused, never wrapped" do
      # 65736 would wrap to 200 in a u16.
      for status <- [65_736, -1, Integer.pow(2, 70)] do
        assert {:error, {:invalid, "the status must be an integer from 0 to 65535"}} =
                 Website.parse_channel(status, "{}")
      end

      assert {:error, {:call, _}} = Website.parse_me(65_736, "{}")
    end
  end

  describe "one result convention" do
    test "a builder that can refuse returns a tuple either way" do
      assert {:ok, %{"url" => "https://kick.com/api/v2/channels/somechannel"}} =
               Website.channel_request("somechannel")

      assert {:error, {:invalid, _}} = Website.channel_request("..")
      assert {:ok, %{"method" => "GET"}} = Website.me_request("s", "somechannel")
      assert {:error, {:invalid, _}} = Website.me_request("s", "..")
      assert {:error, {:invalid, _}} = Website.timeout_request("s", "somechannel", "u", 20_000)
      assert {:ok, _} = PublicApi.top_livestreams_request("t")
      assert {:ok, [_]} = PublicApi.unsubscribe_requests("t", ["01J0000000000000000000000"])
      assert {:error, {:invalid, _}} = PublicApi.subscriptions_request("t\r\nx: y")
      assert {:ok, "https://" <> _} = Events.emote_url("37226")
      assert {:error, {:invalid, _}} = Events.emote_url("../x")
    end

    test "a builder that cannot fail returns the request" do
      assert %{"method" => "POST", "url" => "https://" <> _} = Realtime.token_request("c")
      assert %{"method" => "GET"} = Website.history_request(1)
    end

    test "requests have tuple headers, from the NIF and from call/2 alike" do
      {:ok, ours} = Website.me_request("s", "somechannel")
      assert {:ok, ^ours} = Kicksdk.call("website.me_request", %{session: "s", channel_slug: "somechannel"})
      assert Enum.all?(ours["headers"], &match?({_, _}, &1))

      {:ok, [r]} = Kicksdk.call("public_api.livestreams_requests", %{token: "t", user_ids: [1]})
      assert Enum.all?(r["headers"], &match?({_, _}, &1))
    end
  end

  describe "endpoints" do
    test "point any part of Kick elsewhere, through the NIF and call/2 alike" do
      assert %{"url" => "http://localhost:9/rt/" <> _} =
               Realtime.token_request("c", %{realtime: "http://localhost:9/rt"})

      assert {:ok, %{"url" => "http://localhost:9/w/" <> _}} =
               Website.me_request("s", "somechannel", %{"website" => "http://localhost:9/w", api: nil})
    end

    test "with an unknown key or a value that is not a URL raise, and send nothing to kick.com" do
      for builder <- [
            fn e -> Realtime.token_request("c", e) end,
            fn e -> Website.channel_request("somechannel", e) end,
            fn e -> Website.me_request("s", "somechannel", e) end,
            fn e -> PublicApi.top_livestreams_request("t", endpoints: e) end
          ] do
        assert_raise ArgumentError, ~r/unknown key :realtme/, fn ->
          builder.(%{realtme: "http://localhost:9"})
        end

        assert_raise ArgumentError, ~r/:api must be a URL string/, fn -> builder.(%{api: 9}) end
        assert_raise ArgumentError, ~r/must be a map/, fn -> builder.("http://localhost:9") end

        # A misspelt key is refused even when its value is nil.
        assert_raise ArgumentError, ~r/unknown key :realtme/, fn -> builder.(%{realtme: nil}) end
        assert_raise ArgumentError, ~r/unknown key "apii"/, fn -> builder.(%{"apii" => nil}) end

        # A struct is not a map of endpoints.
        assert_raise ArgumentError, ~r/got a struct/, fn -> builder.(URI.parse("http://localhost:9")) end
      end
    end

    test "take atom or string keys, and nil for Kick's own" do
      assert %{"url" => "https://web.kick.com/" <> _} = Realtime.token_request("c", %{realtime: nil})
      assert %{"url" => "https://web.kick.com/" <> _} = Realtime.token_request("c", %{"realtime" => nil})

      assert %{"url" => "http://localhost:9/" <> _} =
               Realtime.token_request("c", %{"realtime" => "http://localhost:9"})
    end

    test "are checked by the NIF too" do
      assert_raise ArgumentError, ~r/unknown key `realtme`/, fn ->
        Kicksdk.Native.token_request(~s({"realtme":"http://localhost:9"}), "c")
      end

      assert_raise ArgumentError, ~r/`api` must be a URL string/, fn ->
        Kicksdk.Native.token_request(~s({"api":9}), "c")
      end

      assert_raise ArgumentError, ~r/unknown key `realtme`/, fn ->
        Kicksdk.Native.token_request(~s({"realtme":null}), "c")
      end

      assert %{"url" => "https://web.kick.com/" <> _} =
               Kicksdk.Native.token_request(~s({"realtime":null}), "c")
    end
  end

  test "integers come back exact to 64 bits" do
    assert {:ok, %{"user_id" => 18_446_744_073_709_551_615}} =
             Kicksdk.call("webhooks.parse", %{
               event_type: "channel.followed",
               body: ~s({"broadcaster":{"user_id":1},"follower":{"user_id":18446744073709551615}})
             })
             |> then(fn {:ok, e} -> {:ok, e["follower"]} end)
  end
end
