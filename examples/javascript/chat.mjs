// Prints a Kick channel's chat:  node chat.mjs <channel>
// Build the package first: scripts/build-js.sh
//
// Runs until Ctrl-C. A dropped connection comes back by itself; when Kick
// refuses this client (a ban, a refused token), it says so and tries again
// as a new client after a long wait (5 minutes, growing to 30).

import { readFileSync } from 'node:fs';
import * as kick from '../../bindings/js/index.js';

await kick.init(readFileSync(new URL('../../bindings/js/pkg/kicksdk_bg.wasm', import.meta.url)));

const slug = process.argv[2];
if (!slug) {
  console.error('usage: node chat.mjs <channel>');
  process.exit(1);
}

let channel;
try {
  const req = kick.channelRequest(slug);
  const res = await fetch(req.url, { headers: { 'user-agent': 'Mozilla/5.0' } });
  channel = kick.parseChannel(res.status, await res.text());
} catch (e) {
  console.error(`${slug}: ${e.message}`);
  process.exit(1);
}

const chat = kick.connectChat({
  chatroomId: channel.chatroom_id,
  channelId: channel.id,
  onEvent: ({ event }) => {
    if (event.type === 'chat_message') console.log(`${event.sender.username}: ${event.content}`);
  },
  onDrop: (reason, { terminal, waitMs }) => {
    const wait = waitMs >= 60_000 ? `${Math.round(waitMs / 60_000)} min` : `${(waitMs / 1000).toFixed(1)} s`;
    if (terminal) console.error(`Kick refused this client (${reason}): trying again in ${wait}`);
    else console.error(`(reconnecting in ${wait}: ${reason})`);
  }
});

process.on('SIGINT', () => {
  chat.close();
  process.exit();
});
