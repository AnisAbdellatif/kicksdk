defmodule Kicksdk.Native do
  @moduledoc false

  # The NIF comes precompiled from the GitHub release matching this version,
  # checked against checksum-Elixir.Kicksdk.Native.exs (written when the
  # package is published). It is built from source instead - which needs Rust
  # and the optional :rustler dependency - when:
  #
  #   * KICKSDK_BUILD=1 (or true) is set;
  #   * there is no checksum file: a checkout of the repository;
  #   * no precompiled NIF exists for this machine's target.
  #
  # A download that fails on a supported target stops the build with a
  # message; set KICKSDK_BUILD=1 to build from source instead.
  #
  # KICKSDK_BASE_URL points the download at a mirror of the release assets
  # (the checksums still apply).

  version = Mix.Project.config()[:version]
  root = Path.expand("../..", __DIR__)
  nif_versions = ["2.15"]
  targets = RustlerPrecompiled.Config.default_targets()

  # native/ is the vendored Cargo workspace in a published package; in the
  # repository the crate is at the top level.
  source =
    if File.dir?(Path.join(root, "native/crates/kicksdk-nif")),
      do: "native/crates/kicksdk-nif",
      else: "../../crates/kicksdk-nif"

  # This machine as rustler_precompiled describes a target (its own helper
  # for this is private).
  parts = :erlang.system_info(:system_architecture) |> List.to_string() |> String.split("-")

  system =
    if length(parts) in [3, 4],
      do: Map.new(Enum.zip([:arch, :vendor, :os, :abi], parts)),
      else: %{}

  here = %{
    os_type: :os.type(),
    target_system: system,
    word_size: :erlang.system_info(:wordsize),
    nif_version: hd(nif_versions)
  }

  supported? = match?({:ok, _}, RustlerPrecompiled.target(here, targets, nif_versions))

  force_build =
    System.get_env("KICKSDK_BUILD") in ["1", "true"] or
      not File.exists?(Path.join(root, "checksum-Elixir.Kicksdk.Native.exs")) or
      not supported?

  use RustlerPrecompiled,
    otp_app: :kicksdk,
    crate: "kicksdk_nif",
    path: source,
    base_url:
      System.get_env("KICKSDK_BASE_URL") ||
        "https://github.com/AnisAbdellatif/kicksdk/releases/download/v#{version}",
    version: version,
    targets: targets,
    nif_versions: nif_versions,
    force_build: force_build

  defp err, do: :erlang.nif_error(:nif_not_loaded)

  def parse_event(_name, _data_json), do: err()
  def parse_content(_text, _combine), do: err()
  def emote_url(_id), do: err()

  def chat_channels(_chatroom, _channel), do: err()
  def chat_connection_request(_e, _channel, _client_id), do: err()
  def connection_request(_e, _client_id), do: err()
  def parse_connection(_status, _body), do: err()
  def token_request(_e, _client_id), do: err()
  def parse_token(_status, _body), do: err()

  def session_new(_channels), do: err()
  def session_open(_s, _token, _now), do: err()
  def session_receive(_s, _text, _now), do: err()
  def session_refresh(_s, _token, _now), do: err()
  def session_tick(_s, _now), do: err()
  def session_closed(_s), do: err()
  def session_deadline(_s), do: err()
  def session_subscribed(_s, _channel), do: err()
  def session_connected(_s), do: err()

  def app_token_request(_e, _client_id, _secret), do: err()
  def parse_app_token(_status, _body), do: err()
  def livestreams_requests(_e, _token, _ids), do: err()
  def parse_livestreams(_status, _body), do: err()
  def channels_by_ids_request(_e, _token, _ids), do: err()
  def channels_by_slugs_request(_e, _token, _slugs), do: err()
  def parse_channels(_status, _body), do: err()
  def public_key_request(_e), do: err()
  def parse_public_key(_status, _body), do: err()
  def subscribe_request(_e, _token, _broadcaster, _events), do: err()
  def subscriptions_request(_e, _token), do: err()
  def parse_subscriptions(_status, _body), do: err()
  def unsubscribe_requests(_e, _token, _ids), do: err()

  def channel_request(_e, _slug), do: err()
  def parse_channel(_status, _body), do: err()
  def history_request(_e, _channel), do: err()
  def parse_history(_status, _body), do: err()
  def channel_user_request(_e, _slug, _username), do: err()
  def parse_channel_user(_status, _body), do: err()

  def verify_webhook(_pem, _headers, _body), do: err()
  def call(_name, _args_json), do: err()
  def parse_webhook(_event_type, _body), do: err()
end
