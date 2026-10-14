defmodule Kicksdk.WebhooksTest do
  use ExUnit.Case, async: true

  alias Kicksdk.Webhooks

  @dir Path.expand("../../../fixtures/signing", __DIR__)
  @pem File.read!(Path.join(@dir, "test-public-key.pem"))
  @body File.read!(Path.join(@dir, "body.json"))
  @id "01M38M9K3YRKV8E6Q68C18ZWG4"
  @timestamp "2026-09-24T02:35:42Z"
  @sent DateTime.to_unix(elem(DateTime.from_iso8601(@timestamp), 1))

  defp headers(signature \\ String.trim(File.read!(Path.join(@dir, "signature.b64")))) do
    [
      {"Kick-Event-Message-Id", @id},
      {"Kick-Event-Message-Timestamp", @timestamp},
      {"Kick-Event-Signature", signature},
      {"Kick-Event-Type", "channel.followed"},
      {"Kick-Event-Version", "1"}
    ]
  end

  test "a signed, fresh delivery verifies and reads; a changed body does not" do
    {:ok, key} = Webhooks.public_key(@pem)

    assert {:ok,
            %{"event" => %{"type" => "followed", "follower" => %{"user_id" => 900_000_003}}, "headers" => h}} =
             Webhooks.verify(key, headers(), @body, now: @sent + 2)

    assert h["message_id"] == @id

    assert {:error, {:webhook, "signature does not match"}} =
             Webhooks.verify(key, headers(), @body <> " ", now: @sent)
  end

  test "the key is read once and reused, or read from a PEM on each call" do
    {:ok, key} = Webhooks.public_key(@pem)
    for _ <- 1..3, do: assert({:ok, _} = Webhooks.verify(key, headers(), @body, now: @sent))
    assert {:ok, _} = Webhooks.verify(@pem, Map.new(headers()), @body, now: @sent)
    assert {:error, {:webhook, "unreadable public key"}} = Webhooks.public_key("not a key")
    assert {:error, {:webhook, "unreadable public key"}} = Webhooks.public_key(<<255>>)

    assert {:error, {:webhook, "unreadable public key"}} =
             Webhooks.verify("not a key", headers(), @body, now: @sent)
  end

  test "a delivery must be fresh, unless the check is skipped on purpose" do
    # The fixture was signed in 2026-09: today's clock finds it stale.
    assert {:error, {:webhook, "the delivery's timestamp is " <> _}} = Webhooks.verify(@pem, headers(), @body)
    assert {:error, {:webhook, _}} = Webhooks.verify(@pem, headers(), @body, now: @sent + 301)
    assert {:error, {:webhook, _}} = Webhooks.verify(@pem, headers(), @body, now: @sent - 301)
    assert {:ok, _} = Webhooks.verify(@pem, headers(), @body, now: @sent + 301, tolerance_secs: 400)
    assert {:ok, _} = Webhooks.verify(@pem, headers(), @body, skip_freshness: true)

    assert_raise ArgumentError, fn -> Webhooks.verify(@pem, headers(), @body, now: -1) end

    assert_raise ArgumentError, fn ->
      Webhooks.verify(@pem, headers(), @body, now: 1, skip_freshness: true)
    end

    assert_raise ArgumentError, fn -> Webhooks.verify(@pem, headers(), @body, nwo: 1) end
  end

  test "a body is verified byte for byte, UTF-8 or not" do
    # A throwaway key, to sign a body that is not UTF-8.
    private = :public_key.generate_key({:rsa, 2048, 65_537})
    {:RSAPrivateKey, _, n, e, _, _, _, _, _, _, _} = private
    pem = :public_key.pem_encode([:public_key.pem_entry_encode(:SubjectPublicKeyInfo, {:RSAPublicKey, n, e})])
    body = <<"{\"broadcaster\":{\"user_id\":1},\"follower\":{\"username\":\"", 255, "\"}}">>
    sign = fn b -> Base.encode64(:public_key.sign("#{@id}.#{@timestamp}." <> b, :sha256, private)) end

    # The signature holds over the raw bytes; the body is then not JSON.
    assert {:error, {:json, _}} = Webhooks.verify(pem, headers(sign.(body)), body, now: @sent)
    # One byte changed and it does not.
    changed = binary_part(body, 0, byte_size(body) - 1) <> "]"

    assert {:error, {:webhook, "signature does not match"}} =
             Webhooks.verify(pem, headers(sign.(body)), changed, now: @sent)
  end

  test "Kick's headers must be text; others are not read" do
    bad = List.keyreplace(headers(), "Kick-Event-Version", 0, {"Kick-Event-Version", <<255>>})

    assert {:error, {:webhook, "header Kick-Event-Version is not UTF-8"}} =
             Webhooks.verify(@pem, bad, @body, now: @sent)

    assert {:ok, _} = Webhooks.verify(@pem, [{"x-other", <<255>>} | headers()], @body, now: @sent)

    assert {:error, {:webhook, "missing header" <> _}} =
             Webhooks.verify(@pem, tl(headers()), @body, now: @sent)
  end

  test "a signed body must be the shape of its claimed type" do
    retyped = List.keyreplace(headers(), "Kick-Event-Type", 0, {"Kick-Event-Type", "kicks.gifted"})
    assert {:error, {:shape, _}} = Webhooks.verify(@pem, retyped, @body, now: @sent)
  end

  test "parse reads raw bytes, and fails on what is not JSON" do
    assert {:ok, %{"type" => "followed"}} = Webhooks.parse("channel.followed", @body)
    assert {:error, {:json, _}} = Webhooks.parse("channel.followed", <<"{\"a\":\"", 255, "\"}">>)
    assert {:error, {:invalid, _}} = Webhooks.parse(<<255>>, @body)
  end
end
