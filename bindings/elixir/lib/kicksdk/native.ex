defmodule Kicksdk.Native do
  @moduledoc false
  use Rustler, otp_app: :kicksdk, crate: "kicksdk_nif"

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
  def pin_request(_e, _session, _slug, _message_json, _minutes), do: err()

  def verify_webhook(_pem, _headers, _body), do: err()
  def parse_webhook(_event_type, _body), do: err()
end
