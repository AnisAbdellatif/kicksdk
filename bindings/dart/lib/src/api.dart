// The core's functions, by module. Results are Kick-shaped JSON (maps with
// snake_case keys, as Kick writes them), except requests, which are
// [KickRequest]s ready for an HTTP client.

import 'dart:convert';

import 'native.dart' as native;

typedef Json = Map<String, dynamic>;

/// A request to send, with whatever HTTP client the app uses.
class KickRequest {
  final String method;
  final Uri url;
  final List<(String, String)> headers;
  final String? body;

  KickRequest(this.method, this.url, this.headers, this.body);

  factory KickRequest.fromJson(Object? json) {
    final m = json as Json;
    return KickRequest(m['method'] as String, Uri.parse(m['url'] as String), [
      for (final h in m['headers'] as List) (h[0] as String, h[1] as String),
    ], m['body'] as String?);
  }

  @override
  String toString() => '$method $url';
}

Map<String, Object?> _with(Map<String, String>? endpoints, Map<String, Object?> args) =>
    endpoints == null ? args : {...args, 'endpoints': endpoints};

KickRequest _req(String name, Map<String, Object?> args, Map<String, String>? e) =>
    KickRequest.fromJson(native.call(name, _with(e, args)));

List<KickRequest> _reqs(String name, Map<String, Object?> args, Map<String, String>? e) => [
  for (final r in native.call(name, _with(e, args)) as List) KickRequest.fromJson(r),
];

T _parse<T>(String name, int status, String body) => native.call(name, {'status': status, 'body': body}) as T;

/// Kick's chat-feed events, typed: `{"type": "chat_message", ...}`.
abstract final class Events {
  /// Reads one event: its name and its `data` as Kick sent it.
  static Json parse(String name, Object? data) => native.call('events.parse', {'name': name, 'data': data}) as Json;
}

/// Chat message text.
abstract final class Content {
  /// Splits text into `{"type": "text"}` and `{"type": "emote"}` parts.
  static List<Json> parse(String text, {bool combine = false}) =>
      (native.call('content.parse', {'text': text, 'combine': combine}) as List).cast<Json>();

  /// An emote's image.
  static String emoteUrl(String id) => native.call('content.emote_url', {'id': id}) as String;
}

/// Kick's realtime chat feed. Its server only accepts a WebSocket with
/// `Origin: https://kick.com`; `connectChat` in `package:kicksdk/io.dart`
/// sends it.
abstract final class Realtime {
  static const requiredOrigin = 'https://kick.com';

  /// The feeds for one chat, the chat feed first.
  static List<String> chatChannels(int chatroomId, [int? channelId]) =>
      (native.call('realtime.chat_channels', {'chatroom_id': chatroomId, 'channel_id': channelId}) as List)
          .cast<String>();

  static KickRequest chatConnectionRequest(int channelId, String clientId, {Map<String, String>? endpoints}) =>
      _req('realtime.chat_connection_request', {'channel_id': channelId, 'client_id': clientId}, endpoints);

  static KickRequest connectionRequest(String clientId, {Map<String, String>? endpoints}) =>
      _req('realtime.connection_request', {'client_id': clientId}, endpoints);

  /// `[{"provider": "centrifugo", "url": ...}]`.
  static List<Json> parseConnection(int status, String body) =>
      _parse<List>('realtime.parse_connection', status, body).cast<Json>();

  static KickRequest tokenRequest(String clientId, {Map<String, String>? endpoints}) =>
      _req('realtime.token_request', {'client_id': clientId}, endpoints);

  static String parseToken(int status, String body) => _parse('realtime.parse_token', status, body);

  /// The least to wait, in ms, before reconnecting after the socket closed
  /// with `code`: 300000 (5 minutes) for Centrifugo's terminal codes
  /// (3500-3999, 4500-4999: a bad token, a ban, a refused request; then
  /// reconnect with a new client id), 0 (the ordinary backoff) for anything
  /// else (a network drop is 1006). For a session's `close` action, read its
  /// `retry_after_ms` instead.
  static int retryAfterMs(int code) => native.call('realtime.retry_after_ms', {'code': code}) as int;
}

/// One connection's worth of Kick's Centrifugo protocol. Each method returns
/// the actions to carry out: `{"action": "send", "text": ...}`,
/// `{"action": "close", "code", "reason", "terminal", "retry_after_ms"}`
/// (close the socket, call [closed], wait
/// `Backoff.nextDelayFor(retry_after_ms, random)` and reconnect, with a new
/// client id if `terminal`: never stop for good on one, docs/KICK.md §6.1),
/// `refresh_token` (fetch a token for the same client id and pass it to
/// [refresh]; if that fails, carry on: Kick ends the connection when the old
/// token runs out), `connected`, `subscribed`, `unsubscribed`,
/// `subscribe_failed`, `event`. `subscribed` for the chat feed (the first of
/// [Realtime.chatChannels]) is when to call [Backoff.reset]. Times are
/// milliseconds.
class Session {
  final native.NativeSession _s;

  Session(List<String> channels) : _s = native.NativeSession(channels);

  List<Json> _actions(String m, Map<String, Object?> a) => (_s.call(m, a) as List).cast<Json>();

  List<Json> open(String token, int now) => _actions('open', {'token': token, 'now': now});
  List<Json> receive(String text, int now) => _actions('receive', {'text': text, 'now': now});
  List<Json> refresh(String token, int now) => _actions('refresh', {'token': token, 'now': now});
  List<Json> tick(int now) => _actions('tick', {'now': now});
  List<Json> closed() => _actions('closed', const {});
  int? get deadline => _s.call('deadline') as int?;
  bool isSubscribed(String channel) => _s.call('is_subscribed', {'channel': channel}) as bool;
  bool get isConnected => _s.call('is_connected') as bool;

  /// Frees the native session now rather than when this is collected.
  void dispose() => _s.close();
}

/// Reconnect delays: 1 s doubling to 30 s, back to 1 s once connected, and
/// a slower stage for terminal closes (5, 10, 20, then 30 minutes, see
/// [nextDelayFor]). The core's `realtime::Backoff`, in milliseconds: a first
/// wait under 1 ms counts as 1 ms, and a longest wait below the first is
/// raised to it.
class Backoff {
  final Duration initial;
  final Duration max;
  late Duration _next = initial;
  // Terminal closes since the last reset.
  var _terminalCloses = 0;

  Backoff({Duration initial = const Duration(seconds: 1), Duration max = const Duration(seconds: 30)})
    : this._(initial < _min ? _min : initial, max);

  Backoff._(this.initial, Duration max) : max = max < initial ? initial : max;

  static const _min = Duration(milliseconds: 1);

  // The longest terminal wait, as a multiple of the shortest: 5 → 30 minutes.
  static const _terminalCap = 6;

  // The most milliseconds a Duration holds (it counts microseconds in an
  // int); terminal waits saturate here, where the core's saturate at 2^64-1.
  static const _maxMs = 0x7fffffffffffffff ~/ 1000;

  /// How long to wait before the next attempt; the one after waits longer.
  Duration next() {
    final d = _next;
    final doubled = _next * 2;
    _next = doubled > max ? max : doubled;
    return d;
  }

  /// [next], spread: somewhere from half of it to all of it, by `random`,
  /// drawn uniformly from [0, 1) (`Random().nextDouble()`), so that clients
  /// dropped together do not all come back at once. Out of range is
  /// clamped; NaN counts as 0. Never less than 1 ms.
  Duration nextDelayJittered(double random) {
    final ms = next().inMilliseconds;
    final half = ms ~/ 2;
    return Duration(milliseconds: (half + _spread(ms - half, random)).clamp(1, ms));
  }

  /// How long to wait after a close that asks for at least `retryAfterMs`
  /// (a session `close` action's `retry_after_ms`, or [Realtime.retryAfterMs]
  /// of the socket's close code), spread by `random` from [0, 1) (out of
  /// range clamped, NaN as 0).
  ///
  /// - 0, an ordinary drop: [nextDelayJittered].
  /// - More, a terminal close: `retryAfterMs` doubled for each terminal close
  ///   before this one since the last [reset], at most six times
  ///   `retryAfterMs`, plus `random` times a quarter of that. Never less than
  ///   `retryAfterMs`. The ordinary stage is left as it was.
  ///
  /// With Kick's 300000 and `random` 0: 5, 10, 20, 30, 30 … minutes; with
  /// `random` 0.5, an eighth more each. A negative `retryAfterMs` is a
  /// [RangeError].
  Duration nextDelayFor(int retryAfterMs, double random) {
    RangeError.checkNotNegative(retryAfterMs, 'retryAfterMs');
    if (retryAfterMs == 0) return nextDelayJittered(random);
    final doubled = 1 << (_terminalCloses < 3 ? _terminalCloses : 3);
    final factor = doubled < _terminalCap ? doubled : _terminalCap;
    _terminalCloses++;
    final least = retryAfterMs < _maxMs ? retryAfterMs : _maxMs;
    final wait = least > _maxMs ~/ factor ? _maxMs : least * factor;
    final total = wait + _spread(wait ~/ 4, random);
    return Duration(milliseconds: total < _maxMs ? total : _maxMs);
  }

  /// The chat feed is subscribed: the next failure starts from the
  /// beginning, and so does the next terminal close.
  void reset() {
    _next = initial;
    _terminalCloses = 0;
  }

  // `random` (from [0, 1); out of range clamped, NaN as 0) times `span`.
  static int _spread(int span, double random) {
    final r = random.isNaN ? 0.0 : random.clamp(0.0, 1.0);
    final x = (span * r).floor();
    return x < span ? x : span;
  }
}

/// Kick's official API (`api.kick.com/public/v1`).
abstract final class PublicApi {
  static KickRequest tokenRequest(String clientId, String clientSecret, {Map<String, String>? endpoints}) =>
      _req('public_api.token_request', {'client_id': clientId, 'client_secret': clientSecret}, endpoints);
  static Json parseToken(int status, String body) => _parse('public_api.parse_token', status, body);

  /// One request per 50 user ids.
  static List<KickRequest> livestreamsRequests(String token, List<int> userIds, {Map<String, String>? endpoints}) =>
      _reqs('public_api.livestreams_requests', {'token': token, 'user_ids': userIds}, endpoints);
  static List<Json> parseLivestreams(int status, String body) =>
      _parse<List>('public_api.parse_livestreams', status, body).cast<Json>();

  static KickRequest channelsByIdsRequest(String token, List<int> userIds, {Map<String, String>? endpoints}) =>
      _req('public_api.channels_by_ids_request', {'token': token, 'user_ids': userIds}, endpoints);
  static KickRequest channelsBySlugsRequest(String token, List<String> slugs, {Map<String, String>? endpoints}) =>
      _req('public_api.channels_by_slugs_request', {'token': token, 'slugs': slugs}, endpoints);
  static List<Json> parseChannels(int status, String body) =>
      _parse<List>('public_api.parse_channels', status, body).cast<Json>();

  /// The top live streams, overall or in a category or language: up to 100.
  static KickRequest topLivestreamsRequest(
    String token, {
    int? categoryId,
    String? language,
    String sort = 'viewer_count',
    int limit = 100,
    Map<String, String>? endpoints,
  }) => _req('public_api.top_livestreams_request', {
    'token': token,
    'category_id': categoryId,
    'language': language,
    'sort': sort,
    'limit': limit,
  }, endpoints);

  /// Subscribes the app to webhook events (name, version) for one channel.
  static KickRequest subscribeRequest(
    String token,
    int broadcasterUserId,
    List<(String, int)> events, {
    Map<String, String>? endpoints,
  }) => _req('public_api.subscribe_request', {
    'token': token,
    'broadcaster_user_id': broadcasterUserId,
    'events': [
      for (final (name, version) in events) {'name': name, 'version': version},
    ],
  }, endpoints);
  static KickRequest subscriptionsRequest(String token, {Map<String, String>? endpoints}) =>
      _req('public_api.subscriptions_request', {'token': token}, endpoints);
  static List<Json> parseSubscriptions(int status, String body) =>
      _parse<List>('public_api.parse_subscriptions', status, body).cast<Json>();

  /// One request per 50 subscription ids.
  static List<KickRequest> unsubscribeRequests(String token, List<String> ids, {Map<String, String>? endpoints}) =>
      _reqs('public_api.unsubscribe_requests', {'token': token, 'ids': ids}, endpoints);

  static KickRequest publicKeyRequest({Map<String, String>? endpoints}) =>
      _req('public_api.public_key_request', const {}, endpoints);
  static String parsePublicKey(int status, String body) => _parse('public_api.parse_public_key', status, body);
}

/// kick.com's own endpoints (`kick.com/api/v2`): undocumented, a grey area
/// under Kick's terms. Treat a failure as a gap.
abstract final class Website {
  static KickRequest channelRequest(String slug, {Map<String, String>? endpoints}) =>
      _req('website.channel_request', {'slug': slug}, endpoints);
  static Json parseChannel(int status, String body) => _parse('website.parse_channel', status, body);

  static KickRequest historyRequest(int channelId, {Map<String, String>? endpoints}) =>
      _req('website.history_request', {'channel_id': channelId}, endpoints);
  static Json parseHistory(int status, String body) => _parse('website.parse_history', status, body);

  static KickRequest channelUserRequest(String channelSlug, String username, {Map<String, String>? endpoints}) =>
      _req('website.channel_user_request', {'channel_slug': channelSlug, 'username': username}, endpoints);
  static Json parseChannelUser(int status, String body) => _parse('website.parse_channel_user', status, body);

  // Acting as a viewer: `session` is kick.com's `session_token`. Never log it.

  static KickRequest meRequest(String session, String channelSlug, {Map<String, String>? endpoints}) =>
      _req('website.me_request', {'session': session, 'channel_slug': channelSlug}, endpoints);
  static Json parseMe(int status, String body) => _parse('website.parse_me', status, body);

  static KickRequest banRequest(
    String session,
    String channelSlug,
    String username, {
    Map<String, String>? endpoints,
  }) => _req('website.ban_request', {'session': session, 'channel_slug': channelSlug, 'username': username}, endpoints);
  static KickRequest timeoutRequest(
    String session,
    String channelSlug,
    String username,
    int minutes, {
    Map<String, String>? endpoints,
  }) => _req('website.timeout_request', {
    'session': session,
    'channel_slug': channelSlug,
    'username': username,
    'minutes': minutes,
  }, endpoints);
  static KickRequest unbanRequest(
    String session,
    String channelSlug,
    String username, {
    Map<String, String>? endpoints,
  }) =>
      _req('website.unban_request', {'session': session, 'channel_slug': channelSlug, 'username': username}, endpoints);
  static KickRequest deleteMessageRequest(
    String session,
    int chatroomId,
    String messageId, {
    Map<String, String>? endpoints,
  }) => _req('website.delete_message_request', {
    'session': session,
    'chatroom_id': chatroomId,
    'message_id': messageId,
  }, endpoints);

  /// Pins `message` (the feed's data for it, whole) for `minutes`.
  static KickRequest pinRequest(
    String session,
    String channelSlug,
    Object? message,
    int minutes, {
    Map<String, String>? endpoints,
  }) => _req('website.pin_request', {
    'session': session,
    'channel_slug': channelSlug,
    'message': message,
    'minutes': minutes,
  }, endpoints);
  static KickRequest unpinRequest(String session, String channelSlug, {Map<String, String>? endpoints}) =>
      _req('website.unpin_request', {'session': session, 'channel_slug': channelSlug}, endpoints);

  /// A message, or a reply when `replyTo` is
  /// `{message_id, content, sender_id, sender_username}`.
  static KickRequest sendRequest(
    String session,
    int chatroomId,
    String content,
    String messageRef, {
    Map<String, Object?>? replyTo,
    Map<String, String>? endpoints,
  }) => _req('website.send_request', {
    'session': session,
    'chatroom_id': chatroomId,
    'content': content,
    'message_ref': messageRef,
    'reply_to': replyTo,
  }, endpoints);

  /// Throws a [KickException] unless the acting call worked.
  static void parseAction(int status, String body) => _parse<Object?>('website.parse_action', status, body);
}

/// A webhook body as the core takes it: text as `body`, bytes as
/// `body_base64` (verified byte for byte, UTF-8 or not).
Map<String, Object?> _body(Object body) => switch (body) {
  String text => {'body': text},
  List<int> bytes => {'body_base64': base64Encode(bytes)},
  _ => throw ArgumentError.value(body, 'body', 'must be a String or a List<int> of bytes'),
};

/// Kick's official webhooks.
abstract final class Webhooks {
  /// The default for `tolerance` in [verify]: 5 minutes.
  static const defaultTolerance = Duration(seconds: 300);

  /// Verifies and reads a delivery: `{"headers": ..., "event": ...}`.
  ///
  /// `body` is the request body exactly as received: a String, or the raw
  /// bytes (a `List<int>`, e.g. a `Uint8List`), which is the safer of the
  /// two since a body that is not UTF-8 only verifies as bytes.
  ///
  /// The delivery's timestamp must be within `tolerance` (default 5
  /// minutes) of `now` (default: this machine's clock), so a captured
  /// delivery cannot be replayed later. `skipFreshness: true` turns that
  /// off, to re-check a stored delivery on purpose. A valid signature only
  /// means Kick sent this body to some app; deduplicate on
  /// `kick-event-message-id` too (docs/KICK.md §4.3).
  static Json verify(
    String publicKeyPem,
    List<(String, String)> headers,
    Object body, {
    DateTime? now,
    Duration? tolerance,
    bool skipFreshness = false,
  }) =>
      native.call('webhooks.verify', {
            'public_key_pem': publicKeyPem,
            'headers': [
              for (final (k, v) in headers) [k, v],
            ],
            ..._body(body),
            if (skipFreshness)
              'skip_freshness': true
            else ...{
              'now': (now ?? DateTime.now()).millisecondsSinceEpoch ~/ 1000,
              'tolerance_secs': ?tolerance?.inSeconds,
            },
          })
          as Json;

  /// Reads a delivery's body as the type its `kick-event-type` header
  /// names, without verifying it. `body` as for [verify].
  static Json parse(String eventType, Object body) =>
      native.call('webhooks.parse', {'event_type': eventType, ..._body(body)}) as Json;
}
