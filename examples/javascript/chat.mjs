// Prints a Kick channel's chat:  node chat.mjs xqc
// Build the package first: scripts/build-js.sh

import { readFileSync } from 'node:fs';
import * as kick from '../../bindings/js/index.js';

await kick.init(readFileSync(new URL('../../bindings/js/pkg/kicksdk_bg.wasm', import.meta.url)));

const req = kick.channelRequest(process.argv[2] || 'xqc');
const res = await fetch(req.url, { headers: { 'user-agent': 'Mozilla/5.0' } });
const channel = kick.parseChannel(res.status, await res.text());

kick.connectChat({
  chatroomId: channel.chatroom_id,
  channelId: channel.id,
  onEvent: ({ event }) => {
    if (event.type === 'chat_message') console.log(`${event.sender.username}: ${event.content}`);
  }
});
