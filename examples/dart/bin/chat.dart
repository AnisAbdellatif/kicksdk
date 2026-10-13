// Prints a Kick channel's chat:  dart run bin/chat.dart <channel>
import 'dart:convert';
import 'dart:io';

import 'package:kicksdk/io.dart';

Future<void> main(List<String> args) async {
  if (args.isEmpty) {
    stderr.writeln('usage: dart run bin/chat.dart <channel>');
    exit(64);
  }
  final req = Website.channelRequest(args.first);
  final res = await (await (HttpClient()..userAgent = 'Mozilla/5.0').openUrl(req.method, req.url)).close();
  final channel = Website.parseChannel(res.statusCode, await res.transform(utf8.decoder).join());

  connectChat(
    chatroomId: channel['chatroom_id'] as int,
    channelId: channel['id'] as int,
    onEvent: (e) {
      final event = e['event'] as Map<String, dynamic>;
      if (event['type'] == 'chat_message') print('${event['sender']['username']}: ${event['content']}');
    },
  );
}
