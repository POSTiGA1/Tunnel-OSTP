import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:shared_preferences/shared_preferences.dart';

import '../models/ostp_profile.dart';
import '../models/share_link.dart';
import '../models/subscription.dart';
import 'home_screen.dart';
import 'qr_scanner_screen.dart';
import 'ssh_setup.dart';

const welcomedKey = 'welcomed';

/// Whether to open on the welcome screen: never set up, nothing to connect to.
bool needsWelcome(SharedPreferences prefs) =>
    prefs.getBool(welcomedKey) != true &&
    decodeProfiles(prefs.getString('profiles_json')).isEmpty &&
    SubscriptionStore(prefs).load().isEmpty;

/// First run: a link or subscription someone gave you, or your own server
/// set up over SSH.
class WelcomeScreen extends StatefulWidget {
  final SharedPreferences prefs;
  const WelcomeScreen({super.key, required this.prefs});

  @override
  State<WelcomeScreen> createState() => _WelcomeScreenState();
}

class _WelcomeScreenState extends State<WelcomeScreen> {
  final _link = TextEditingController();
  bool _busy = false;

  @override
  void dispose() {
    _link.dispose();
    super.dispose();
  }

  Future<void> _finish() async {
    await widget.prefs.setBool(welcomedKey, true);
    if (!mounted) return;
    Navigator.pushReplacement(context, MaterialPageRoute(builder: (_) => HomeScreen(prefs: widget.prefs)));
  }

  Future<void> _addLink([String? scanned]) async {
    final raw = (scanned ?? _link.text).trim();
    if (raw.isEmpty) return;
    final messenger = ScaffoldMessenger.of(context);
    setState(() => _busy = true);
    try {
      if (isSubscriptionUrl(raw)) {
        final n = await SubscriptionStore(widget.prefs).add(raw);
        messenger.showSnackBar(SnackBar(content: Text('Subscription added: $n profile(s)')));
      } else {
        final l = ShareLink.parse(raw);
        final profiles = decodeProfiles(widget.prefs.getString('profiles_json'));
        profiles.add(OstpProfile(
          id: DateTime.now().millisecondsSinceEpoch.toString(),
          name: l.name ?? l.host,
          serverAddr: l.server,
          accessKey: l.key,
          transportMode: l.transport,
          active: !profiles.any((p) => p.active),
          tls: l.tls,
          tlsSni: l.sni ?? '',
          tlsInsecure: l.insecure,
          wsPath: l.path ?? '',
        ));
        await widget.prefs.setString('profiles_json', encodeProfiles(profiles));
      }
      await _finish();
    } catch (e) {
      messenger.showSnackBar(SnackBar(content: Text(e.toString().replaceFirst('Exception: ', '').replaceFirst('FormatException: ', ''))));
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Widget _card({required IconData icon, required String title, required String hint, required Widget child}) => Card(
        margin: const EdgeInsets.only(bottom: 16),
        child: Padding(
          padding: const EdgeInsets.all(16),
          child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
            Row(children: [
              Icon(icon, size: 22),
              const SizedBox(width: 12),
              Expanded(
                child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
                  Text(title, style: const TextStyle(fontSize: 16, fontWeight: FontWeight.w700)),
                  const SizedBox(height: 2),
                  Text(hint, style: const TextStyle(color: Colors.white54, fontSize: 12)),
                ]),
              ),
            ]),
            const SizedBox(height: 14),
            child,
          ]),
        ),
      );

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: const Text('OSTP', style: TextStyle(fontWeight: FontWeight.w800, letterSpacing: 2.5)),
        actions: [TextButton(onPressed: _finish, child: const Text('Skip'))],
      ),
      body: SafeArea(
        child: ListView(padding: const EdgeInsets.fromLTRB(20, 8, 20, 24), children: [
          const Text("Let's get started", style: TextStyle(fontSize: 26, fontWeight: FontWeight.w800)),
          const SizedBox(height: 8),
          const Text('Connect with a link you were given, or set up your own server: all it takes is a VPS and its SSH login.',
              style: TextStyle(color: Colors.white54, height: 1.4)),
          const SizedBox(height: 20),
          _card(
            icon: Icons.link,
            title: 'I have a link',
            hint: 'An ostp:// link or a subscription URL',
            child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
              TextField(
                controller: _link,
                maxLines: 3,
                minLines: 1,
                autocorrect: false,
                style: const TextStyle(fontFamily: 'monospace', fontSize: 13),
                decoration: InputDecoration(
                  hintText: 'ostp://… or https://…/sub/…',
                  hintStyle: const TextStyle(color: Colors.white24),
                  filled: true,
                  fillColor: Theme.of(context).colorScheme.surface,
                  border: OutlineInputBorder(borderRadius: BorderRadius.circular(12), borderSide: BorderSide.none),
                ),
              ),
              const SizedBox(height: 10),
              Row(children: [
                IconButton(
                  tooltip: 'Paste',
                  icon: const Icon(Icons.content_paste),
                  onPressed: () async {
                    final d = await Clipboard.getData(Clipboard.kTextPlain);
                    if (d?.text != null) _link.text = d!.text!.trim();
                  },
                ),
                IconButton(
                  tooltip: 'Scan a QR code',
                  icon: const Icon(Icons.qr_code_scanner),
                  onPressed: () async {
                    final v = await Navigator.push<String>(context, MaterialPageRoute(builder: (_) => const QRScannerScreen()));
                    if (v != null && v.isNotEmpty) {
                      _link.text = v;
                      _addLink(v);
                    }
                  },
                ),
                const Spacer(),
                FilledButton(onPressed: _busy ? null : () => _addLink(), child: const Text('Add')),
              ]),
            ]),
          ),
          _card(
            icon: Icons.dns_outlined,
            title: 'I have a server',
            hint: 'OSTP is installed over SSH; no Linux knowledge needed',
            child: SshSetupForm(prefs: widget.prefs, onInstalled: (_) => _finish()),
          ),
        ]),
      ),
    );
  }
}
