// The core's functions, by module. Results are Kick-shaped JSON (maps with
// snake_case keys, as Kick writes them), except requests, which are
// [KickRequest]s ready for an HTTP client.

import 'dart:convert';
import 'dart:math';

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

  /// `METHOD url`, safe to log: a `token` in the query (as
  /// [OAuth.revokeRequest] sends it) is shown as `[redacted]`.
  @override
  String toString() => '$method ${'$url'.replaceAll(RegExp(r'(?<=[?&])token=[^&#]*'), 'token=[redacted]')}';
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
/// `subscribe_failed` (with `temporary`: a refusal for now, which a `close`
/// that is not terminal follows, so the reconnect subscribes again),
/// `event`. A call that panicked leaves the session refusing every call
/// (a [KickException] of kind `panic`) until [closed]. `subscribed` for the chat feed (the first of
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

/// Kick's official API (`api.kick.com/public/v1` and `/public/v2`).
abstract final class PublicApi {
  static KickRequest tokenRequest(String clientId, String clientSecret, {Map<String, String>? endpoints}) =>
      _req('public_api.token_request', {'client_id': clientId, 'client_secret': clientSecret}, endpoints);
  static Json parseToken(int status, String body) => _parse('public_api.parse_token', status, body);

  /// `/public/v1/users/livestreams`: one request per 100 user ids.
  static List<KickRequest> livestreamsRequests(String token, List<int> userIds, {Map<String, String>? endpoints}) =>
      _reqs('public_api.livestreams_requests', {'token': token, 'user_ids': userIds}, endpoints);
  static List<Json> parseLivestreams(int status, String body) =>
      _parse<List>('public_api.parse_livestreams', status, body).cast<Json>();

  /// One page of the streams live now (`/public/v2/livestreams`), in up to 25
  /// categories and 25 languages (none: all). [limit] is 1 to 1000; [cursor]
  /// is the previous page's `next_cursor`. Kick has no sort: streams come
  /// oldest first.
  static KickRequest livestreamsPageRequest(
    String token, {
    List<int>? categoryIds,
    List<String>? languageCodes,
    int? limit,
    String? cursor,
    Map<String, String>? endpoints,
  }) => _req('public_api.livestreams_page_request', {
    'token': token,
    'category_ids': categoryIds,
    'language_codes': languageCodes,
    'limit': limit,
    'cursor': cursor,
  }, endpoints);

  /// A [livestreamsPageRequest] answer: `{data: [...], next_cursor: ...}`.
  static Json parseLivestreamsPage(int status, String body) =>
      _parse('public_api.parse_livestreams_page', status, body);

  static KickRequest channelsByIdsRequest(String token, List<int> userIds, {Map<String, String>? endpoints}) =>
      _req('public_api.channels_by_ids_request', {'token': token, 'user_ids': userIds}, endpoints);
  static KickRequest channelsBySlugsRequest(String token, List<String> slugs, {Map<String, String>? endpoints}) =>
      _req('public_api.channels_by_slugs_request', {'token': token, 'slugs': slugs}, endpoints);
  static List<Json> parseChannels(int status, String body) =>
      _parse<List>('public_api.parse_channels', status, body).cast<Json>();

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

  /// Users by id, up to 50 (`/public/v1/users`), with an app or a user
  /// token. No ids, with a user token: the token's own user.
  static KickRequest usersRequest(String token, {List<int>? userIds, Map<String, String>? endpoints}) =>
      _req('public_api.users_request', {'token': token, 'user_ids': userIds}, endpoints);

  /// `[{user_id, name, profile_picture, email}]`; `email` only for a user
  /// token with `user:read`, for its own user.
  static List<Json> parseUsers(int status, String body) =>
      _parse<List>('public_api.parse_users', status, body).cast<Json>();

  /// One page of categories (`/public/v2/categories`): by [names] and
  /// [tags], or by [ids]; none of them, all categories. [limit] is 1 to 1000
  /// (Kick's default: 25); [cursor] is the previous page's `next_cursor`.
  static KickRequest categoriesRequest(
    String token, {
    List<String>? names,
    List<String>? tags,
    List<int>? ids,
    int? limit,
    String? cursor,
    Map<String, String>? endpoints,
  }) => _req('public_api.categories_request', {
    'token': token,
    'names': names,
    'tags': tags,
    'ids': ids,
    'limit': limit,
    'cursor': cursor,
  }, endpoints);

  /// A [categoriesRequest] answer: `{data: [...], next_cursor: ...}`.
  static Json parseCategories(int status, String body) => _parse('public_api.parse_categories', status, body);

  /// How many streams are live on Kick, with an app or a user token.
  static KickRequest livestreamStatsRequest(String token, {Map<String, String>? endpoints}) =>
      _req('public_api.livestream_stats_request', {'token': token}, endpoints);

  /// `{total_count}`.
  static Json parseLivestreamStats(int status, String body) =>
      _parse('public_api.parse_livestream_stats', status, body);

  /// The top [top] Kicks gifters (1 to 100; Kick's default: 10) of the
  /// token's own channel. A user token with `kicks:read`.
  static KickRequest kicksLeaderboardRequest(String token, {int? top, Map<String, String>? endpoints}) =>
      _req('public_api.kicks_leaderboard_request', {'token': token, 'top': top}, endpoints);

  /// `{lifetime, month, week}`, each `[{user_id, username, rank, gifted_amount}]`.
  static Json parseKicksLeaderboard(int status, String body) =>
      _parse('public_api.parse_kicks_leaderboard', status, body);

  /// Throws a [KickException] unless Kick says the action worked: for the
  /// answers with nothing to read (a deleted message, a ban, a channel
  /// update, a deleted reward, enrolling in ads, updated drops claims).
  static void parseDone(int status, String body) => _parse<Object?>('public_api.parse_done', status, body);
}

/// Kick's user OAuth (`id.kick.com`): sign a user in with PKCE, refresh,
/// revoke and introspect their token. See docs/KICK.md §2.
abstract final class OAuth {
  /// A new PKCE pair, `{verifier, challenge}`, from 32 bytes of
  /// `Random.secure()`. Keep the verifier until the user comes back.
  static Json newPkce() => pkce(_random32());

  /// A new `state` for [authorizeUrl]: 32 bytes of `Random.secure()`, as
  /// base64url without padding. Check that the redirect brings it back.
  static String newState() => _random32();

  static String _random32() {
    final r = Random.secure();
    return base64Url.encode([for (var i = 0; i < 32; i++) r.nextInt(256)]).replaceAll('=', '');
  }

  /// The PKCE pair for [random]: 32 to 96 bytes from a CSPRNG, as base64url
  /// without padding. [newPkce] draws them.
  static Json pkce(String random) => native.call('oauth.pkce', {'random': random}) as Json;

  /// The PKCE pair for a verifier made elsewhere (or kept from before).
  static Json pkceFromVerifier(String verifier) =>
      native.call('oauth.pkce_from_verifier', {'verifier': verifier}) as Json;

  /// Where to send the user's browser to approve the app. [redirectUri] is
  /// exactly the one in the app's settings; [state] is [newState]'s; the
  /// challenge is [verifier]'s.
  static String authorizeUrl(
    String clientId,
    String redirectUri,
    List<String> scopes,
    String state,
    String verifier, {
    Map<String, String>? endpoints,
  }) =>
      native.call(
            'oauth.authorize_url',
            _with(endpoints, {
              'client_id': clientId,
              'redirect_uri': redirectUri,
              'scopes': scopes,
              'state': state,
              'verifier': verifier,
            }),
          )
          as String;

  /// A user token for the `code` on the redirect, with the PKCE verifier
  /// and the same [redirectUri].
  static KickRequest codeRequest(
    String clientId,
    String clientSecret,
    String redirectUri,
    String code,
    String codeVerifier, {
    Map<String, String>? endpoints,
  }) => _req('oauth.code_request', {
    'client_id': clientId,
    'client_secret': clientSecret,
    'redirect_uri': redirectUri,
    'code': code,
    'code_verifier': codeVerifier,
  }, endpoints);

  /// A new user token (and refresh token) for a refresh token.
  static KickRequest refreshRequest(
    String clientId,
    String clientSecret,
    String refreshToken, {
    Map<String, String>? endpoints,
  }) => _req('oauth.refresh_request', {
    'client_id': clientId,
    'client_secret': clientSecret,
    'refresh_token': refreshToken,
  }, endpoints);

  /// `{access_token, refresh_token, expires_in, refresh_expires_in,
  /// token_type, scopes}`. Never log the tokens.
  static Json parseUserToken(int status, String body) => _parse('oauth.parse_user_token', status, body);

  /// Revokes an access or refresh token; [tokenTypeHint] is `access_token`
  /// or `refresh_token`.
  static KickRequest revokeRequest(String token, {String? tokenTypeHint, Map<String, String>? endpoints}) =>
      _req('oauth.revoke_request', {'token': token, 'token_type_hint': tokenTypeHint}, endpoints);

  /// Throws a [KickException] unless the token was revoked.
  static void parseRevoke(int status, String body) => _parse<Object?>('oauth.parse_revoke', status, body);

  /// Whether a token is active, and what it may do.
  static KickRequest introspectRequest(String token, {Map<String, String>? endpoints}) =>
      _req('oauth.introspect_request', {'token': token}, endpoints);

  /// `{active, client_id, exp, scopes, token_type}`.
  static Json parseIntrospect(int status, String body) => _parse('oauth.parse_introspect', status, body);

  /// The scopes Kick documents, e.g. `chat:write`.
  static List<String> scopes() => (native.call('oauth.scopes') as List).cast<String>();
}

/// Chat as a user or the app's bot (`/public/v1/chat`): a user token with
/// `chat:write`; deleting needs `moderation:chat_message:manage`.
abstract final class Chat {
  /// Sends [content] as the token's user into [broadcasterUserId]'s chat,
  /// or, with `null`, as the app's bot into the token's own channel; a
  /// reply to [replyToMessageId] if given.
  static KickRequest sendRequest(
    String token,
    int? broadcasterUserId,
    String content, {
    String? replyToMessageId,
    Map<String, String>? endpoints,
  }) => _req('chat.send_request', {
    'token': token,
    'sender': broadcasterUserId == null ? 'bot' : {'user': broadcasterUserId},
    'content': content,
    'reply_to_message_id': replyToMessageId,
  }, endpoints);

  /// `{is_sent, message_id}`.
  static Json parseSend(int status, String body) => _parse('chat.parse_send', status, body);

  /// Deletes a message by id; read the answer with [PublicApi.parseDone].
  static KickRequest deleteRequest(String token, String messageId, {Map<String, String>? endpoints}) =>
      _req('chat.delete_request', {'token': token, 'message_id': messageId}, endpoints);
}

/// Bans and timeouts (`/public/v1/moderation/bans`): a user token with
/// `moderation:ban`. Read the answers with [PublicApi.parseDone].
abstract final class Moderation {
  /// Bans [userId] from [broadcasterUserId]'s chat for good.
  static KickRequest banRequest(
    String token,
    int broadcasterUserId,
    int userId, {
    String? reason,
    Map<String, String>? endpoints,
  }) => _req('moderation.ban_request', {
    'token': token,
    'broadcaster_user_id': broadcasterUserId,
    'user_id': userId,
    'reason': reason,
  }, endpoints);

  /// Times [userId] out for [minutes], 1 to 10080 (a week).
  static KickRequest timeoutRequest(
    String token,
    int broadcasterUserId,
    int userId,
    int minutes, {
    String? reason,
    Map<String, String>? endpoints,
  }) => _req('moderation.timeout_request', {
    'token': token,
    'broadcaster_user_id': broadcasterUserId,
    'user_id': userId,
    'minutes': minutes,
    'reason': reason,
  }, endpoints);

  /// Lifts a ban or a timeout.
  static KickRequest unbanRequest(String token, int broadcasterUserId, int userId, {Map<String, String>? endpoints}) =>
      _req('moderation.unban_request', {
        'token': token,
        'broadcaster_user_id': broadcasterUserId,
        'user_id': userId,
      }, endpoints);
}

/// The token's own channel (`PATCH /public/v1/channels`): a user token with
/// `channel:write`.
abstract final class Channels {
  /// Sets the stream's category, custom tags (`[]` clears them; at most 10)
  /// and title; what is `null` stays. Read the answer with
  /// [PublicApi.parseDone].
  static KickRequest updateRequest(
    String token, {
    int? categoryId,
    List<String>? customTags,
    String? streamTitle,
    Map<String, String>? endpoints,
  }) => _req('channels.update_request', {
    'token': token,
    'category_id': categoryId,
    'custom_tags': customTags,
    'stream_title': streamTitle,
  }, endpoints);
}

/// Channel point rewards and their redemptions, on the token's own channel:
/// a user token with `channel:rewards:read` (reading) or
/// `channel:rewards:write`. Ids are ULIDs; only the app that made a reward
/// may change it or settle its redemptions.
abstract final class Rewards {
  static KickRequest listRequest(String token, {Map<String, String>? endpoints}) =>
      _req('rewards.list_request', {'token': token}, endpoints);

  /// The channel's rewards, as [parseReward] reads one.
  static List<Json> parseRewards(int status, String body) =>
      _parse<List>('rewards.parse_rewards', status, body).cast<Json>();

  /// A new reward. [fields]: `title` and `cost` (needed), `description`,
  /// `background_color`, `is_enabled`, `is_user_input_required`,
  /// `should_redemptions_skip_request_queue`.
  static KickRequest createRequest(String token, Map<String, Object?> fields, {Map<String, String>? endpoints}) =>
      _req('rewards.create_request', {'token': token, 'fields': fields}, endpoints);

  /// Changes the [fields] given (as for [createRequest], and `is_paused`).
  static KickRequest updateRequest(
    String token,
    String id,
    Map<String, Object?> fields, {
    Map<String, String>? endpoints,
  }) => _req('rewards.update_request', {'token': token, 'id': id, 'fields': fields}, endpoints);

  /// A reward, from [createRequest] or [updateRequest].
  static Json parseReward(int status, String body) => _parse('rewards.parse_reward', status, body);

  /// Read the answer with [PublicApi.parseDone].
  static KickRequest deleteRequest(String token, String id, {Map<String, String>? endpoints}) =>
      _req('rewards.delete_request', {'token': token, 'id': id}, endpoints);

  /// Redemptions, grouped by reward: of one reward or all, with one
  /// [status] (`pending`, Kick's default; `accepted`; `rejected`), from
  /// [cursor] (a page's `next_cursor`).
  static KickRequest redemptionsRequest(
    String token, {
    String? rewardId,
    String? status,
    String? cursor,
    Map<String, String>? endpoints,
  }) => _req('rewards.redemptions_request', {
    'token': token,
    'reward_id': rewardId,
    'status': status,
    'cursor': cursor,
  }, endpoints);

  /// Redemptions by id, whatever their status.
  static KickRequest redemptionsByIdsRequest(
    String token,
    List<String> ids, {
    String? cursor,
    Map<String, String>? endpoints,
  }) => _req('rewards.redemptions_by_ids_request', {'token': token, 'ids': ids, 'cursor': cursor}, endpoints);

  /// `{rewards: [{reward, redemptions}], next_cursor}`.
  static Json parseRedemptions(int status, String body) => _parse('rewards.parse_redemptions', status, body);

  /// Accepts up to 25 redemptions; read the answer with [parseFailures].
  static KickRequest acceptRequest(String token, List<String> ids, {Map<String, String>? endpoints}) =>
      _req('rewards.accept_request', {'token': token, 'ids': ids}, endpoints);

  /// Rejects up to 25 redemptions (refunding them); read the answer with
  /// [parseFailures].
  static KickRequest rejectRequest(String token, List<String> ids, {Map<String, String>? endpoints}) =>
      _req('rewards.reject_request', {'token': token, 'ids': ids}, endpoints);

  /// The redemptions that were not accepted or rejected: `[{id, reason}]`.
  static List<Json> parseFailures(int status, String body) =>
      _parse<List>('rewards.parse_failures', status, body).cast<Json>();
}

/// Ads on the token's own channel: a user token with `ads:write` (`ads:read`
/// is enough for [statusRequest]).
abstract final class Ads {
  /// Starts an ad break of [seconds] (7 to 90). [id] is a UUID the app
  /// makes up for it.
  static KickRequest adBreakRequest(String token, String id, int seconds, {Map<String, String>? endpoints}) =>
      _req('ads.ad_break_request', {'token': token, 'id': id, 'seconds': seconds}, endpoints);

  /// `{ad_break_id, remaining_ad_breaks, ...}`.
  static Json parseAdBreak(int status, String body) => _parse('ads.parse_ad_break', status, body);

  static KickRequest statusRequest(String token, {Map<String, String>? endpoints}) =>
      _req('ads.status_request', {'token': token}, endpoints);

  /// `{opted_in, remaining_ad_breaks, limits, ad_breaks, ...}`.
  static Json parseStatus(int status, String body) => _parse('ads.parse_status', status, body);

  /// Opts the channel in to ads; read the answer with [PublicApi.parseDone].
  static KickRequest enrollRequest(String token, {Map<String, String>? endpoints}) =>
      _req('ads.enroll_request', {'token': token}, endpoints);
}

/// Drops reward claims (`/public/v1/drops/claims`): an **app** token of the
/// organization running the campaigns.
abstract final class Drops {
  /// One page of claims, filtered by what is given. [limit] is 1 to 1000;
  /// [cursor] is the previous page's `cursor`.
  static KickRequest claimsRequest(
    String token, {
    String? campaignId,
    String? claimId,
    int? userId,
    String? externalStatus,
    int? limit,
    String? cursor,
    Map<String, String>? endpoints,
  }) => _req('drops.claims_request', {
    'token': token,
    'filter': {
      'campaign_id': campaignId,
      'claim_id': claimId,
      'user_id': userId,
      'external_status': externalStatus,
      'limit': limit,
      'cursor': cursor,
    },
  }, endpoints);

  /// `{claims: [...], cursor}`.
  static Json parseClaims(int status, String body) => _parse('drops.parse_claims', status, body);

  /// Sets claims' `external_status` (claim id, status), up to 100. Read the
  /// answer with [PublicApi.parseDone].
  static KickRequest updateClaimsRequest(
    String token,
    List<(String, String)> updates, {
    Map<String, String>? endpoints,
  }) => _req('drops.update_claims_request', {
    'token': token,
    'updates': [
      for (final (claimId, status) in updates) {'claim_id': claimId, 'external_status': status},
    ],
  }, endpoints);
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
