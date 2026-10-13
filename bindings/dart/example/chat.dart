// Prints a channel's chat:  dart run example/chat.dart xqc
import 'dart:convert';
import 'dart:io';

import 'package:kicksdk/io.dart';

Future<void> main(List<String> args) async {
  final slug = args.isEmpty ? 'xqc' : args.first;
  final http = HttpClient()..userAgent = 'Mozilla/5.0';
  final r = Website.channelRequest(slug);
  final req = await http.openUrl(r.method, r.url);
  final res = await req.close();
  final channel = Website.parseChannel(res.statusCode, await res.transform(utf8.decoder).join());
  http.close();
  stdout.writeln('$slug: chatroom ${channel['chatroom_id']}, ${channel['live'] == true ? 'live' : 'offline'}');

  final chat = connectChat(
    chatroomId: channel['chatroom_id'] as int,
    channelId: channel['id'] as int,
    onState: (connected, subscribed) =>
        stdout.writeln('[${connected ? 'connected' : 'down'}: ${subscribed.length} feeds]'),
    onEvent: (e) {
      final event = e['event'] as Map<String, dynamic>;
      if (event['type'] == 'chat_message') {
        stdout.writeln('${event['sender']['username']}: ${event['content']}');
      }
    },
  );
  if (args.length > 1) {
    await Future<void>.delayed(Duration(seconds: int.parse(args[1])));
    chat.close();
  }
}
