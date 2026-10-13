// Prints a channel's chat as it arrives.
//
//   node examples/node-chat.mjs xqc
//
// Run `scripts/build-js.sh` first. Node sends the Origin Kick's realtime
// server wants; the same code in a browser only works on kick.com.

import { readFileSync } from 'node:fs';
import * as kick from '../bindings/js/index.js';

await kick.init(readFileSync(new URL('../bindings/js/pkg/kicksdk_bg.wasm', import.meta.url)));

const slug = process.argv[2] || 'xqc';
// kick.com's website API refuses a full browser User-Agent from something that
// is not a browser; a bare one passes.
const req = kick.channelRequest(slug);
const res = await fetch(req.url, { headers: { ...Object.fromEntries(req.headers), 'user-agent': 'Mozilla/5.0' } });
const channel = kick.parseChannel(res.status, await res.text());
console.log(`${slug}: channel ${channel.id}, chatroom ${channel.chatroom_id}, ${channel.live ? 'live' : 'offline'}`);

const chat = kick.connectChat({
  chatroomId: channel.chatroom_id,
  channelId: channel.id,
  onState: (s) => console.log(`[${s.connected ? 'connected' : 'disconnected'}: ${s.subscribed.length} feeds]`),
  onDrop: (reason) => console.log(`[dropped: ${reason}]`),
  onEvent: ({ event }) => {
    if (event.type === 'chat_message') console.log(`${event.sender.username}: ${event.content}`);
    else console.log(`[${event.type}]`);
  }
});

const seconds = Number(process.argv[3] || 0);
if (seconds) setTimeout(() => (chat.close(), process.exit(0)), seconds * 1000);
