defmodule KicksdkTest do
  # The NIF against the shared fixtures in the repository's fixtures/: the
  # same recordings the Rust and JavaScript tests read.
  use ExUnit.Case, async: true

  alias Kicksdk.{Events, PublicApi, Realtime, Website}
  alias Kicksdk.Realtime.Session

  @fixtures Path.expand("../../../fixtures", __DIR__)

  defp files(dir, part \\ ""),
    do:
      Path.wildcard(Path.join([@fixtures, dir, "*"]))
      |> Enum.filter(&String.contains?(&1, part))
      |> Enum.sort()

  defp response(path) do
    %{"response" => %{"status" => status, "body" => body}} = path |> File.read!() |> JSON.decode!()
    {status, body}
  end

  test "events are typed with string keys, unknown ones kept by name" do
    assert %{"type" => "host", "host_username" => "A", "viewers" => 3} =
             Events.parse("App\\Events\\StreamHostEvent", ~s({"host_username":"A","number_viewers":3}))

    assert Events.parse("NewThing", "{}") == %{"type" => "unknown", "name" => "NewThing"}
    assert [%{"type" => "emote", "count" => 2}] = Events.parse_content("[emote:1:X] [emote:1:X]", true)
  end

  test "requests carry Kick's origin and tuple headers; endpoints can be overridden" do
    r = Realtime.chat_connection_request(668, "c1", %{realtime: "http://localhost:9/rt"})
    assert r["method"] == "POST"
    assert r["url"] == "http://localhost:9/rt/channels/668/chat/connection"
    assert {"origin", "https://kick.com"} in r["headers"]
    assert {:ok, [_, _, _]} = PublicApi.livestreams_requests("t", Enum.to_list(1..120))
    assert {:error, {:invalid, _}} = PublicApi.channels_by_ids_request("t", [])
  end

  test "livestreams polls are read" do
    for path <- files("public_api", "livestreams-poll") do
      {status, body} = response(path)
      assert {:ok, streams} = PublicApi.parse_livestreams(status, body)
      for s <- streams, do: assert(String.ends_with?(s["started_at"], "Z"))
    end
  end

  test "one unknown slug fails a /channels batch" do
    for path <- files("public_api", "-channels") do
      case response(path) do
        {400, body} -> assert {:error, {:status, 400, _}} = PublicApi.parse_channels(400, body)
        {status, body} -> assert {:ok, [_ | _]} = PublicApi.parse_channels(status, body)
      end
    end
  end

  test "website channels give the chatroom and followers" do
    for path <- files("v2") do
      {status, body} = response(path)

      assert {:ok, %{"chatroom_id" => chatroom, "followers_count" => followers}} =
               Website.parse_channel(status, body)

      assert chatroom > 0 and is_integer(followers)
    end
  end

  test "a recorded realtime session replays through Session" do
    for path <- files("realtime") do
      records = path |> File.read!() |> String.split("\n", trim: true) |> Enum.map(&JSON.decode!/1)
      sent = for %{"direction" => "out", "frame" => f} <- records, do: f

      channels =
        for f <- sent,
            line <- String.split(f, "\n"),
            %{"subscribe" => %{"channel" => c}} <- [JSON.decode!(line)],
            do: c

      session = Session.new(channels)
      opened = for %{"action" => "send", "text" => t} <- Session.open(session, "[redacted]", 0), do: t

      actions =
        for %{"direction" => "in", "frame" => f, "ms" => ms} <- records,
            action <- Session.receive(session, f, ms),
            do: action

      ours = opened ++ for(%{"action" => "send", "text" => t} <- actions, do: t)
      assert ours == sent
      assert Session.subscribed?(session, hd(channels))

      assert Enum.any?(actions, &match?(%{"action" => "event", "event" => %{"type" => "chat_message"}}, &1))

      assert Enum.any?(
               actions,
               &match?(%{"action" => "subscribe_failed", "code" => 103, "channel" => "private-" <> _}, &1)
             )

      assert Session.closed(session) |> Enum.all?(&(&1["action"] == "unsubscribed"))
      refute Session.connected?(session)
    end
  end
end
