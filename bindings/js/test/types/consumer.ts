// A TypeScript consumer of the package, checked with --strict (npm run
// typecheck): the types must accept what the runtime accepts, and refuse
// what it throws on.

import * as kick from '../../index.js';
import type { ChatDrop, ChatState, Endpoints, VerifiedWebhook } from '../../index.js';

export async function main(pem: string, body: Uint8Array, headers: Headers): Promise<void> {
  const loaded: void = await kick.init();
  void loaded;
  await kick.init(new Uint8Array());

  // Optional endpoints, and Partial<> config that carries undefined.
  const config: { realtime?: string } = {};
  const endpoints: Endpoints = { realtime: config.realtime };
  kick.tokenRequest('client');
  kick.tokenRequest('client', endpoints);
  kick.tokenRequest('client', { api: undefined });

  // Ids are numbers; lists are plain (or readonly) arrays.
  const ids: readonly number[] = [1, 2, 3];
  kick.livestreamsRequests('token', ids);
  kick.channelsByIdsRequest('token', [1, 2]);
  const feeds: string[] = kick.chatChannels(668);
  kick.chatChannels(668, 1);
  kick.topLivestreamsRequest('token');
  kick.topLivestreamsRequest('token', undefined, 'en', 'started_at', 10);
  kick.sendRequest('session', 668, 'hi', 'ref');
  kick.sendRequest('session', 668, 'hi', 'ref', { message_id: 'm', sender_id: 1, sender_username: 'someone' });
  kick.subscribeRequest('token', 1, [{ name: 'channel.followed' }]);
  // @ts-expect-error ids are numbers, not numeric strings
  kick.chatChannels('668');
  // @ts-expect-error not a Float64Array any more, but neither strings
  kick.livestreamsRequests('token', ['1']);
  // @ts-expect-error an unknown sort
  kick.topLivestreamsRequest('token', null, null, 'loudest');

  // The realtime pieces.
  const reconnect: boolean = kick.shouldReconnect(1006);
  const backoff = new kick.Backoff();
  const wait: number = backoff.nextDelayJittered(Math.random());
  backoff.free();
  void reconnect;
  void wait;

  // Webhooks: the key once, bodies as text or bytes, freshness options.
  const key = new kick.WebhookPublicKey(pem);
  const verified: VerifiedWebhook = key.verify(headers, body, { now: Math.floor(Date.now() / 1000) });
  const type: string = verified.event.type;
  const id: string = verified.headers.message_id;
  kick.verifyWebhook(pem, [['kick-event-type', 'channel.followed']], '{}', { skipFreshness: true });
  kick.verifyWebhook(pem, { 'kick-event-type': 'channel.followed' }, body);
  kick.verifyWebhook(pem, headers, body.buffer as ArrayBuffer, { now: 1, toleranceSecs: 60 });
  // @ts-expect-error snake_case is the dispatch's spelling, not this one's
  kick.verifyWebhook(pem, headers, body, { tolerance_secs: 60 });
  kick.parseWebhook('channel.followed', body);
  key.free();
  void type;
  void id;

  // connectChat: every option, as a caller would write them.
  const chat = kick.connectChat({
    chatroomId: 668,
    channelId: undefined,
    channels: feeds,
    endpoints: { realtime: config.realtime },
    onEvent: ({ channel, event }) => console.log(channel, event.type),
    onState: (s: ChatState) => console.log(s.connected, s.subscribed, s.stopped),
    onDrop: (reason: string, drop: ChatDrop) => {
      if (!drop.reconnect) console.log('stopped', reason, drop.code);
    },
    onError: (e: unknown) => console.error(e),
    fetch,
    WebSocket,
    now: () => performance.now(),
    timeoutMs: 5_000
  });
  chat.close();
  kick.connectChat({ chatroomId: 668, onEvent() {}, createSocket: (url) => new WebSocket(url) }).close();

  // A WebSocket class shaped like the `ws` package's.
  class WsLike {
    readyState: 0 | 1 | 2 | 3 = 0;
    onopen: ((ev: { target: WsLike }) => void) | null = null;
    onmessage: ((ev: { data: string | Buffer }) => void) | null = null;
    onclose: ((ev: { code: number; reason: string }) => void) | null = null;
    onerror: ((ev: { error: Error }) => void) | null = null;
    constructor(_url: string, _options?: { headers?: Record<string, string> }) {}
    send(_data: string | Buffer, _cb?: (err?: Error) => void): void {}
    close(_code?: number, _reason?: string): void {}
  }
  kick.connectChat({ chatroomId: 668, onEvent() {}, WebSocket: WsLike }).close();
}

type Buffer = Uint8Array;
