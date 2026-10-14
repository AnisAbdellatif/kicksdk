defmodule Kicksdk.Native do
  @moduledoc false

  # The NIF comes precompiled from the GitHub release matching this version,
  # checked against checksum-Elixir.Kicksdk.Native.exs (written when the
  # package is published). It is built from source instead - which needs Rust
  # and the optional :rustler dependency - when:
  #
  #   * KICKSDK_BUILD=1 (or true) is set, or rustler_precompiled's own
  #     `config :rustler_precompiled, :force_build, kicksdk: true` (or
  #     RUSTLER_PRECOMPILED_FORCE_BUILD_ALL);
  #   * there is no checksum file: a checkout of the repository;
  #   * no precompiled NIF exists for this machine's target, or the target
  #     cannot be told (TARGET_ARCH, TARGET_VENDOR, TARGET_OS and TARGET_ABI
  #     override it, as Nerves sets them).
  #
  # A download that fails on a supported target stops the build with a
  # message; set KICKSDK_BUILD=1 to build from source instead.
  #
  # KICKSDK_BASE_URL points the download at a mirror of the release assets
  # (the checksums still apply).
  #
  # These are read when this module compiles, and a change to any of them
  # recompiles it (`__mix_recompile__?/0`). As a dependency, Mix does not
  # look again by itself: run `mix deps.compile kicksdk --force`.

  version = Mix.Project.config()[:version]
  root = Path.expand("../..", __DIR__)
  nif_versions = ["2.15"]
  targets = RustlerPrecompiled.Config.default_targets()
  checksum = Path.join(root, "checksum-Elixir.Kicksdk.Native.exs")

  @env ~w(KICKSDK_BUILD KICKSDK_BASE_URL TARGET_ARCH TARGET_VENDOR TARGET_OS TARGET_ABI
          RUSTLER_PRECOMPILED_FORCE_BUILD_ALL)
  @checksum checksum
  @inputs {Enum.map(@env, &System.get_env/1), File.exists?(checksum)}

  @doc false
  # Mix asks before reusing this module's compiled code.
  def __mix_recompile__?, do: {Enum.map(@env, &System.get_env/1), File.exists?(@checksum)} != @inputs

  # native/ is the vendored Cargo workspace in a published package; in the
  # repository the crate is at the top level.
  source =
    if File.dir?(Path.join(root, "native/crates/kicksdk-nif")),
      do: "native/crates/kicksdk-nif",
      else: "../../crates/kicksdk-nif"

  # This machine as rustler_precompiled describes a target, TARGET_* applied
  # (its own helpers for this are private). An architecture string that is
  # not three or four parts cannot be read: build from source.
  parts = :erlang.system_info(:system_architecture) |> List.to_string() |> String.split("-")
  keys = [arch: "TARGET_ARCH", vendor: "TARGET_VENDOR", os: "TARGET_OS", abi: "TARGET_ABI"]

  detected =
    if length(parts) in [3, 4],
      do: Map.new(Enum.zip(Keyword.keys(keys), parts)),
      else: %{}

  system =
    Enum.reduce(keys, detected, fn {key, var}, acc ->
      case System.get_env(var) do
        nil -> acc
        value -> Map.put(acc, key, value)
      end
    end)

  # As rustler_precompiled does: a Linux target set through TARGET_* without
  # a vendor is "unknown".
  system =
    if system != detected and system[:vendor] == detected[:vendor] and system[:os] == "linux",
      do: Map.put(system, :vendor, "unknown"),
      else: system

  here = %{
    os_type: :os.type(),
    target_system: system,
    word_size: :erlang.system_info(:wordsize),
    nif_version: hd(nif_versions)
  }

  supported? =
    Map.has_key?(system, :arch) and Map.has_key?(system, :os) and
      try do
        match?({:ok, _}, RustlerPrecompiled.target(here, targets, nif_versions))
      rescue
        _ -> false
      end

  # force_build only when this package needs it, so that rustler_precompiled's
  # own `config :rustler_precompiled, :force_build, kicksdk: true` still
  # applies otherwise.
  force =
    if System.get_env("KICKSDK_BUILD") in ["1", "true"] or not File.exists?(checksum) or not supported?,
      do: [force_build: true],
      else: []

  use RustlerPrecompiled,
      [
        otp_app: :kicksdk,
        crate: "kicksdk_nif",
        path: source,
        base_url:
          System.get_env("KICKSDK_BASE_URL") ||
            "https://github.com/AnisAbdellatif/kicksdk/releases/download/v#{version}",
        version: version,
        targets: targets,
        nif_versions: nif_versions
      ] ++ force

  # A NIF should hand its scheduler back within about a millisecond. Those
  # that read input take time in proportion to it (about 20 us per KiB of
  # JSON, 100 us per KiB of message text), so each has a `_dirty` twin on a
  # dirty CPU scheduler, used above this size. Kick's answers and frames are
  # mostly smaller and stay on the normal scheduler.
  @dirty_bytes 4096

  @doc false
  def dirty?(input), do: is_binary(input) and byte_size(input) > @dirty_bytes

  defp err, do: :erlang.nif_error(:nif_not_loaded)

  def parse_event(_name, _data_json), do: err()
  def parse_event_dirty(_name, _data_json), do: err()
  def parse_content(_text, _combine), do: err()
  def parse_content_dirty(_text, _combine), do: err()
  def emote_url(_id), do: err()

  def chat_channels(_chatroom, _channel), do: err()
  def chat_connection_request(_e, _channel, _client_id), do: err()
  def connection_request(_e, _client_id), do: err()
  def parse_connection(_status, _body), do: err()
  def parse_connection_dirty(_status, _body), do: err()
  def token_request(_e, _client_id), do: err()
  def parse_token(_status, _body), do: err()
  def parse_token_dirty(_status, _body), do: err()
  def retry_after_ms(_code), do: err()
  def backoff_new(_initial_ms, _max_ms), do: err()
  def backoff_next(_backoff, _random), do: err()
  def backoff_reset(_backoff), do: err()

  def session_new(_channels), do: err()
  def session_open(_s, _token, _now), do: err()
  def session_receive(_s, _text, _now), do: err()
  def session_receive_dirty(_s, _text, _now), do: err()
  def session_refresh(_s, _token, _now), do: err()
  def session_tick(_s, _now), do: err()
  def session_closed(_s), do: err()
  def session_deadline(_s), do: err()
  def session_subscribed(_s, _channel), do: err()
  def session_connected(_s), do: err()

  def app_token_request(_e, _client_id, _secret), do: err()
  def parse_app_token(_status, _body), do: err()
  def parse_app_token_dirty(_status, _body), do: err()
  def livestreams_requests(_e, _token, _ids), do: err()
  def parse_livestreams(_status, _body), do: err()
  def parse_livestreams_dirty(_status, _body), do: err()
  def channels_by_ids_request(_e, _token, _ids), do: err()
  def channels_by_slugs_request(_e, _token, _slugs), do: err()
  def parse_channels(_status, _body), do: err()
  def parse_channels_dirty(_status, _body), do: err()
  def public_key_request(_e), do: err()
  def parse_public_key(_status, _body), do: err()
  def parse_public_key_dirty(_status, _body), do: err()
  def subscribe_request(_e, _token, _broadcaster, _events), do: err()
  def subscriptions_request(_e, _token), do: err()
  def parse_subscriptions(_status, _body), do: err()
  def parse_subscriptions_dirty(_status, _body), do: err()
  def unsubscribe_requests(_e, _token, _ids), do: err()

  def channel_request(_e, _slug), do: err()
  def parse_channel(_status, _body), do: err()
  def parse_channel_dirty(_status, _body), do: err()
  def history_request(_e, _channel), do: err()
  def parse_history(_status, _body), do: err()
  def parse_history_dirty(_status, _body), do: err()
  def channel_user_request(_e, _slug, _username), do: err()
  def parse_channel_user(_status, _body), do: err()
  def parse_channel_user_dirty(_status, _body), do: err()

  def webhook_public_key(_pem), do: err()
  def verify_webhook(_key, _headers, _body, _freshness), do: err()
  def verify_webhook_dirty(_key, _headers, _body, _freshness), do: err()
  def call(_name, _args_json), do: err()
  def call_dirty(_name, _args_json), do: err()
  def parse_webhook(_event_type, _body), do: err()
  def parse_webhook_dirty(_event_type, _body), do: err()
end
