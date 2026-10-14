// Prints a Kick channel's chat:  dart run bin/chat.dart <channel>
import 'dart:convert';
import 'dart:io';

import 'package:kicksdk/io.dart';

Future<void> main(List<String> args) async {
  if (args.isEmpty) {
    stderr.writeln('usage: dart run bin/chat.dart <channel>');
    exit(64);
  }
  final Map<String, dynamic> channel;
  final http = HttpClient()..userAgent = 'Mozilla/5.0';
  try {
    // Refuses a slug that would change the URL (`..`, an empty one) with
    // kind `invalid`, before anything is sent.
    final req = Website.channelRequest(args.first);
    final res = await (await http.openUrl(req.method, req.url)).close();
    channel = Website.parseChannel(res.statusCode, await res.transform(utf8.decoder).join());
  } on KickException catch (e) {
    stderr.writeln('${args.first}: $e');
    exit(1);
  } finally {
    http.close();
  }

  final chat = connectChat(
    chatroomId: channel['chatroom_id'] as int,
    channelId: channel['id'] as int,
    onEvent: (e) {
      final event = e['event'] as Map<String, dynamic>;
      if (event['type'] == 'chat_message') print('${event['sender']['username']}: ${event['content']}');
    },
    // A reconnect always follows: after a few seconds, or, when Kick refused
    // us (terminal), 5 minutes or more, as a new client.
    onDropInfo: (drop) => stderr.writeln('dropped: $drop'),
    onError: (error, stack) => stderr.writeln('error: $error'),
  );
  ProcessSignal.sigint.watch().first.then((_) => chat.close());
  // Completes only once close() has been called: on Ctrl-C.
  await chat.done;
  exit(0);
}
