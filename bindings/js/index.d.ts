export * from './pkg/kicksdk.js';
import type { Endpoints } from './pkg/kicksdk.js';

/** The Origin Kick's realtime server accepts. */
export const REQUIRED_ORIGIN: 'https://kick.com';

/** Loads the WebAssembly: a URL, a Response, or the .wasm bytes. */
export function init(input?: string | URL | Request | Response | BufferSource): Promise<void>;

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

export interface ChatOptions {
  chatroomId: number;
  channelId?: number;
  channels?: string[];
  onEvent(e: ChatEvent): void;
  onState?(s: { connected: boolean; subscribed: string[] }): void;
  onDrop?(reason: string): void;
  fetch?: typeof fetch;
  createSocket?(url: string): WebSocket;
  /** Kick's URLs to use instead; a key left out or undefined stays Kick's. */
  endpoints?: Endpoints | undefined;
  now?(): number;
}

/** Connects to one channel's chat and keeps it up until `close()`. */
export function connectChat(options: ChatOptions): { close(): void };
