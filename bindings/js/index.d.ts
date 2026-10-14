export * from './pkg/kicksdk.js';
import type { Endpoints } from './pkg/kicksdk.js';

/** The Origin Kick's realtime server accepts. */
export const REQUIRED_ORIGIN: 'https://kick.com';

/**
 * Loads the WebAssembly: a URL, a Response (or a promise of one), the .wasm
 * bytes or a compiled module. Safe to call more than once; after a failure,
 * the next call tries again.
 */
export function init(
  input?: string | URL | Request | Response | Promise<Response> | BufferSource | WebAssembly.Module
): Promise<void>;

export interface ChatEvent {
  /** The feed it came on, e.g. `chatrooms.668.v2`. */
  channel: string;
  /** Kick's event name, e.g. `App\\Events\\ChatMessageEvent`. */
  name: string;
  /** Its data as Kick sent it (usually a JSON string). */
  data: unknown;
  /** The same, typed: `{ type: 'chat_message', … }`. */
  event: { type: string; [key: string]: unknown };
}

/** What connectChat knows of the connection. */
export interface ChatState {
  /** Kick accepted the connection. */
  connected: boolean;
  /** The feeds subscribed now. */
  subscribed: string[];
}

/** Why a connection, or an attempt at one, ended, and when the next one starts. */
export interface ChatDrop {
  /**
   * Kick's disconnect or error code, the socket's close code (1006 for a
   * network drop or a refused handshake), or 0 (a timeout, a failed request,
   * silence).
   */
  code: number;
  /**
   * Kick will not take this client back as it is (a refused token, a ban, a
   * connection limit: codes 3500-3999 and 4500-4999, among others). Not the
   * end: connectChat waits longer (`waitMs`, 5 minutes or more) and comes
   * back as a new guest client. Worth a warning.
   */
  terminal: boolean;
  /**
   * How long connectChat waits before connecting again, ms: the ordinary
   * backoff (0.5 s growing to 30 s, jittered), or after a terminal close 5,
   * 10, 20, then 30 minutes in a row, plus up to a quarter.
   */
  waitMs: number;
}

/**
 * The part of a WebSocket connectChat uses: the browser's, Node's (22 and
 * later), and the `ws` package's all fit.
 */
export interface ChatSocket {
  readonly readyState: number;
  send(data: string): void;
  close(): void;
  onopen: ((ev: any) => any) | null;
  onmessage: ((ev: any) => any) | null;
  onclose: ((ev: any) => any) | null;
  onerror: ((ev: any) => any) | null;
}

/**
 * A WebSocket class. In Node it is given `{ headers: { Origin } }` as its
 * second argument, as Node's own and the `ws` package's take it.
 */
export type ChatSocketConstructor = new (url: string, options?: any) => ChatSocket;

export interface ChatOptions {
  /** The chatroom (website API `chatroom.id`). */
  chatroomId: number;
  /** The channel; also locates the chat's region. */
  channelId?: number | undefined;
  /** The feeds to subscribe to; `chatChannels(chatroomId, channelId)` by default. */
  channels?: readonly string[] | undefined;
  /** An event on a feed. */
  onEvent(e: ChatEvent): void;
  /** The connection or its subscriptions changed. */
  onState?(s: ChatState): void;
  /** A connection, or an attempt at one, ended. */
  onDrop?(reason: string, drop: ChatDrop): void;
  /** A callback above threw. Default: console.error. The connection carries on either way. */
  onError?(error: unknown): void;
  fetch?: typeof fetch;
  /**
   * The WebSocket class, where the runtime has none (Node before 22: pass the
   * `ws` package's). Default: the global WebSocket.
   */
  WebSocket?: ChatSocketConstructor | undefined;
  /** Opens the socket yourself, instead of `WebSocket`. */
  createSocket?(url: string): ChatSocket;
  /** Kick's URLs to use instead; a key left out or undefined stays Kick's. */
  endpoints?: Endpoints | undefined;
  /** The session's clock, ms, only ever going forward. Default: `performance.now()`. */
  now?(): number;
  /**
   * How long a request, or the WebSocket handshake, may take, ms: above 0
   * and at most 2147483647 (setTimeout's limit; there is no "no timeout").
   * Default 10000. Out of range, or not a number: connectChat throws a
   * RangeError.
   */
  timeoutMs?: number | undefined;
}

/**
 * Connects to one channel's chat and keeps it up until `close()`: whatever
 * ends a connection, `onDrop` says why and how long connectChat waits before
 * the next (long after a `terminal` one). No callback fires after `close()`,
 * which may be called more than once, and from inside a callback. Throws at
 * once on a mistake in the options, or when the runtime has no WebSocket and
 * none is given.
 */
export function connectChat(options: ChatOptions): { close(): void };
