import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:shared_preferences/shared_preferences.dart';

import '../models/ostp_profile.dart';
import '../models/share_link.dart';

/// Servers the app installs and manages over SSH. The work is done by the
/// native core (crate ostp-ssh through ostp-jni's `serversCall`); this is the
/// Dart side of that JSON API.
class ServersApi {
  static const _channel = MethodChannel('com.ospab.ostp/vpn');

  /// Passwords and keys typed for servers whose secret is not remembered,
  /// kept for this run of the app only.
  static final Map<String, Map<String, dynamic>> typedAuth = {};

  static Future<dynamic> raw(Map<String, dynamic> request) async {
    final String? reply = await _channel.invokeMethod('serversCall', {'requestJson': jsonEncode(request)});
    if (reply == null) throw ServerError('no answer from the native core');
    final doc = jsonDecode(reply) as Map<String, dynamic>;
    if (doc['error'] != null) throw ServerError(doc['error'] as String);
    return doc['ok'];
  }

  /// A call for server [id]; when its secret is not remembered (or the one
  /// typed was refused) the user is asked for it.
  static Future<dynamic> call(BuildContext context, Map<String, dynamic> request, {Map<String, dynamic>? server}) async {
    final id = request['id'] as String?;
    for (var attempt = 0; attempt < 3; attempt++) {
      try {
        return await raw({...request, if (id != null && typedAuth[id] != null) 'auth': typedAuth[id]});
      } on ServerError catch (e) {
        final needs = e.message.contains('SECRET_NEEDED') || e.message.contains('credential store') || e.message.contains('cannot be opened');
        final refused = typedAuth.containsKey(id) &&
            RegExp(r'did not accept the (password|key)|cannot read the private key|passphrase').hasMatch(e.message);
        if (id == null || (!needs && !refused) || !context.mounted) rethrow;
        typedAuth.remove(id);
        final s = server ?? (await list()).servers.where((x) => x['id'] == id).firstOrNull;
        if (s == null || !context.mounted) rethrow;
        final auth = await askSecret(context, s);
        if (auth == null) throw ServerError('Cancelled');
        typedAuth[id] = auth;
      }
    }
    throw ServerError('Sign-in failed');
  }

  static Future<ServersList> list() async {
    final r = await raw({'op': 'list'}) as Map<String, dynamic>;
    return ServersList(
      (r['servers'] as List).cast<Map<String, dynamic>>(),
      r['can_remember'] as bool? ?? false,
    );
  }

  /// Output lines of running calls after [since]: (lines, next since).
  static Future<(List<Map<String, dynamic>>, int)> lines(int since) async {
    final r = await raw({'op': 'lines', 'since': since}) as Map<String, dynamic>;
    return ((r['lines'] as List).cast<Map<String, dynamic>>(), (r['next'] as num).toInt());
  }

  /// Asks for the password or key of a server whose secret is not remembered.
  static Future<Map<String, dynamic>?> askSecret(BuildContext context, Map<String, dynamic> server) {
    final isKey = server['auth'] == 'key';
    final secret = TextEditingController();
    final passphrase = TextEditingController();
    return showDialog<Map<String, dynamic>>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: Text('Sign in to ${server['name']}'),
        content: SingleChildScrollView(
          child: Column(mainAxisSize: MainAxisSize.min, crossAxisAlignment: CrossAxisAlignment.start, children: [
            Text(
              '${isKey ? 'Private key' : 'Password'} for ${server['user']}@${server['host']} (not remembered on this device).',
              style: const TextStyle(color: Colors.white54, fontSize: 13),
            ),
            const SizedBox(height: 12),
            TextField(
              controller: secret,
              obscureText: !isKey,
              maxLines: isKey ? 5 : 1,
              autofocus: true,
              style: TextStyle(fontFamily: isKey ? 'monospace' : null, fontSize: isKey ? 12 : 16),
              decoration: InputDecoration(hintText: isKey ? '-----BEGIN OPENSSH PRIVATE KEY-----' : 'Password'),
            ),
            if (isKey)
              TextField(
                controller: passphrase,
                obscureText: true,
                decoration: const InputDecoration(hintText: 'Key passphrase (if any)'),
              ),
          ]),
        ),
        actions: [
          TextButton(onPressed: () => Navigator.pop(ctx), child: const Text('Cancel')),
          FilledButton(
            onPressed: () {
              if (secret.text.isEmpty) return;
              Navigator.pop(ctx, {
                'kind': isKey ? 'key' : 'password',
                'secret': secret.text,
                'passphrase': passphrase.text.isEmpty ? null : passphrase.text,
              });
            },
            child: const Text('Sign in'),
          ),
        ],
      ),
    );
  }

  /// Adds the users' links as profiles, skipping ones already there. The
  /// first one becomes active when nothing is. Returns how many were added.
  static Future<int> importUsers(SharedPreferences prefs, String serverName, List<Map<String, dynamic>> users, {int? limit}) async {
    final profiles = decodeProfiles(prefs.getString('profiles_json'));
    var added = 0;
    for (final u in users.take(limit ?? users.length)) {
      final userName = (u['name'] as String?) ?? '';
      for (final link in (u['links'] as List? ?? const [])) {
        ShareLink l;
        try {
          l = ShareLink.parse(link['uri'] as String);
        } catch (_) {
          continue;
        }
        final carrier = l.tls ? 'tls' : l.transport;
        final dup = profiles.any((p) =>
            p.accessKey == l.key && p.serverAddr == l.server && (p.tls ? 'tls' : p.transportMode) == carrier);
        if (dup) continue;
        profiles.add(OstpProfile(
          id: '${DateTime.now().microsecondsSinceEpoch}-$added',
          name: '$serverName${userName.isNotEmpty ? ' · $userName' : ''} · ${carrier.toUpperCase()}',
          serverAddr: l.server,
          accessKey: l.key,
          transportMode: l.transport,
          active: !profiles.any((p) => p.active),
          tls: l.tls,
          tlsSni: l.sni ?? '',
          tlsInsecure: l.insecure,
          wsPath: l.path ?? '',
        ));
        added++;
      }
    }
    if (added > 0) await prefs.setString('profiles_json', encodeProfiles(profiles));
    return added;
  }

  /// Whether the VPN is up through [host]: changes that restart the server
  /// would cut this very connection.
  static Future<bool> connectedThrough(SharedPreferences prefs, String host) async {
    try {
      final running = await _channel.invokeMethod('isRunning');
      if (running != true) return false;
    } catch (_) {
      return false;
    }
    final active = decodeProfiles(prefs.getString('profiles_json')).where((p) => p.active).firstOrNull;
    if (active == null) return false;
    try {
      return ShareLink.parse('ostp://x@${active.serverAddr}').host == host;
    } catch (_) {
      return false;
    }
  }
}

class ServersList {
  final List<Map<String, dynamic>> servers;
  final bool canRemember;
  ServersList(this.servers, this.canRemember);
}

class ServerError implements Exception {
  final String message;
  ServerError(this.message);
  @override
  String toString() => message;
}

String fmtBytes(num? b) {
  if (b == null) return '—';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  var v = b.toDouble();
  var i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return '${v.toStringAsFixed(i == 0 ? 0 : 1)} ${units[i]}';
}

String fmtDuration(num? secs) {
  if (secs == null) return '—';
  final s = secs.toInt();
  final d = s ~/ 86400, h = s % 86400 ~/ 3600, m = s % 3600 ~/ 60;
  if (d > 0) return '$d d $h h';
  if (h > 0) return '$h h $m min';
  return '$m min';
}
