defmodule Kicksdk.DirtyTest do
  # Big inputs go to the NIFs' dirty-scheduler twins: the same answers.
  use ExUnit.Case, async: true

  alias Kicksdk.{Events, Native, PublicApi, Webhooks, Website}
  alias Kicksdk.Realtime.Session

  @stream %{
    "broadcaster_user_id" => 1,
    "channel_id" => 2,
    "slug" => "somechannel",
    "started_at" => "2026-09-23T22:02:09Z",
    "viewer_count" => 3
  }

  test "only inputs over 4 KiB are dirty" do
    refute Native.dirty?(String.duplicate("a", 4096))
    assert Native.dirty?(String.duplicate("a", 4097))
    refute Native.dirty?(nil)
  end

  test "a big answer reads the same through either twin" do
    body = JSON.encode!(%{"data" => List.duplicate(@stream, 200), "message" => "OK"})
    assert Native.dirty?(body)
    assert {:ok, streams} = PublicApi.parse_livestreams(200, body)
    assert length(streams) == 200
    assert {:ok, streams} == Native.parse_livestreams(200, body)
    assert Native.parse_livestreams_dirty(200, ~s({"data":[],"message":"OK"})) == {:ok, []}
    assert {:error, {:status, 500, _}} = Website.parse_history(500, String.duplicate("x", 5000))
  end

  test "big text, frames, bodies and call arguments go dirty and come back the same" do
    text = String.duplicate("hi [emote:1:X] ", 400)
    assert Events.parse_content(text) == Native.parse_content(text, false)

    data = JSON.encode!(%{"host_username" => String.duplicate("a", 5000), "number_viewers" => 3})
    assert %{"type" => "host", "viewers" => 3} = Events.parse("App\\Events\\StreamHostEvent", data)

    s = Session.new(["chatrooms.1.v2"])
    Session.open(s, "t", 0)
    frame = Enum.join([~s({"id":1,"connect":{"client":"c","ping":25}}) | List.duplicate("{}", 2000)], "\n")
    actions = Session.receive(s, frame, 1)
    assert Enum.count(actions, &(&1 == %{"action" => "send", "text" => "{}"})) == 2000

    body =
      ~s({"broadcaster":{"user_id":1},"follower":{"user_id":2,"username":"#{String.duplicate("u", 5000)}"}})

    assert {:ok, %{"type" => "followed"}} = Webhooks.parse("channel.followed", body)
    assert {:error, {:webhook, "missing header" <> _}} = Webhooks.verify("pem", [], body)

    assert {:ok, %{"type" => "followed"}} =
             Kicksdk.call("webhooks.parse", %{event_type: "channel.followed", body: body})
  end
end
