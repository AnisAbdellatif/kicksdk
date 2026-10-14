// Prints a Kick channel's chat:  node chat.mjs <channel>
// Build the package first: scripts/build-js.sh
//
// Runs until Ctrl-C, or until Kick ends the connection for good (a ban, a
// refused token): then it says why and exits with status 1.

import { readFileSync } from 'node:fs';
import * as kick from '../../bindings/js/index.js';

await kick.init(readFileSync(new URL('../../bindings/js/pkg/kicksdk_bg.wasm', import.meta.url)));

const slug = process.argv[2];
if (!slug) {
  console.error('usage: node chat.mjs <channel>');
  process.exit(1);
}

const req = kick.channelRequest(slug);
const res = await fetch(req.url, { headers: { 'user-agent': 'Mozilla/5.0' } });
const channel = kick.parseChannel(res.status, await res.text());

const chat = kick.connectChat({
  chatroomId: channel.chatroom_id,
  channelId: channel.id,
  onEvent: ({ event }) => {
    if (event.type === 'chat_message') console.log(`${event.sender.username}: ${event.content}`);
  },
  onDrop: (reason, { reconnect }) => {
    // A dropped connection comes back by itself; a terminal one does not,
    // and once connectChat has stopped, nothing keeps the process alive.
    console.error(reconnect ? `(reconnecting: ${reason})` : `Kick ended the connection: ${reason}`);
    if (!reconnect) process.exitCode = 1;
  }
});

process.on('SIGINT', () => {
  chat.close();
  process.exit();
});
