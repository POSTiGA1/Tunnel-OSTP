import 'dart:convert';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'app_routing_screen.dart';
import 'logs_screen.dart';
import 'qr_scanner_screen.dart';
import 'package:qr_flutter/qr_flutter.dart';
import 'package:http/http.dart' as http;
import 'package:url_launcher/url_launcher.dart';
import 'package:package_info_plus/package_info_plus.dart';
import '../models/ostp_profile.dart';

class SettingsScreen extends StatefulWidget {
  final SharedPreferences prefs;
  const SettingsScreen({super.key, required this.prefs});

  @override
  State<SettingsScreen> createState() => _SettingsScreenState();
}

class _SettingsScreenState extends State<SettingsScreen> {
  late TextEditingController _localBindCtrl;
  late TextEditingController _dnsCtrl;
  late TextEditingController _mtuCtrl;
  late TextEditingController _domainsCtrl;
  late TextEditingController _ipsCtrl;
  late TextEditingController _processesCtrl;
  late TextEditingController _muxSessionsCtrl;

  bool _debugMode = false;
  bool _muxEnabled = false;
  bool _isCheckingUpdates = false;

  List<OstpProfile> _profiles = [];

  @override
  void initState() {
    super.initState();
    _loadSettings();
  }

  void _loadSettings() {
    _localBindCtrl = TextEditingController(text: widget.prefs.getString('local_bind') ?? '127.0.0.1:1088');
    _dnsCtrl = TextEditingController(text: widget.prefs.getString('dns_server') ?? '1.1.1.1');
    _mtuCtrl = TextEditingController(text: widget.prefs.getString('mtu') ?? '1140');
    _domainsCtrl = TextEditingController(text: widget.prefs.getString('ex_domains') ?? '');
    _ipsCtrl = TextEditingController(text: widget.prefs.getString('ex_ips') ?? '');
    _processesCtrl = TextEditingController(text: widget.prefs.getString('ex_processes') ?? '');
    _debugMode = widget.prefs.getBool('debug_mode') ?? false;
    _muxEnabled = widget.prefs.getBool('mux_enabled') ?? false;
    _muxSessionsCtrl = TextEditingController(text: widget.prefs.getString('mux_sessions') ?? '2');
    _profiles = decodeProfiles(widget.prefs.getString('profiles_json'));
  }

  @override
  void dispose() {
    _saveSettings();
    _localBindCtrl.dispose();
    _dnsCtrl.dispose();
    _mtuCtrl.dispose();
    _domainsCtrl.dispose();
    _ipsCtrl.dispose();
    _processesCtrl.dispose();
    _muxSessionsCtrl.dispose();
    super.dispose();
  }

  void _saveSettings() {
    widget.prefs.setString('local_bind', _localBindCtrl.text.trim());
    widget.prefs.setString('dns_server', _dnsCtrl.text.trim());
    widget.prefs.setString('mtu', _mtuCtrl.text.trim());
    widget.prefs.setString('ex_domains', _domainsCtrl.text.trim());
    widget.prefs.setString('ex_ips', _ipsCtrl.text.trim());
    widget.prefs.setString('ex_processes', _processesCtrl.text.trim());
    widget.prefs.setBool('debug_mode', _debugMode);
    widget.prefs.setBool('mux_enabled', _muxEnabled);
    widget.prefs.setString('mux_sessions', _muxSessionsCtrl.text.trim());
    widget.prefs.setString('profiles_json', encodeProfiles(_profiles));
  }

  void _saveProfiles() {
    widget.prefs.setString('profiles_json', encodeProfiles(_profiles));
  }

  // ── Profile CRUD ─────────────────────────────────────────────────────────

  void _selectActive(OstpProfile p) {
    setState(() {
      for (final other in _profiles) {
        other.active = other.id == p.id;
      }
      _saveProfiles();
    });
  }

  void _importFromLink(String link) {
    if (link.isEmpty) return;
    try {
      if (!link.startsWith('ostp://')) {
        throw Exception('Link must start with ostp://');
      }
      final uri = Uri.parse(link);
      final key = Uri.decodeComponent(uri.userInfo);
      final host = uri.authority.replaceFirst('${uri.userInfo}@', '');
      if (key.isEmpty || host.isEmpty) {
        throw Exception('Incomplete link parameters');
      }
      final type = uri.queryParameters['type'];
      final transportMode = (type == 'tcp' || type == 'http') ? 'uot' : 'udp';
      final name = uri.queryParameters['name'] ?? host;
      final stealthSni = uri.queryParameters['sni'] ?? '';
      final wasEmpty = _profiles.isEmpty;

      setState(() {
        _profiles.add(OstpProfile(
          id: DateTime.now().millisecondsSinceEpoch.toString(),
          name: name,
          serverAddr: host,
          accessKey: key,
          transportMode: transportMode,
          stealthSni: stealthSni,
          active: wasEmpty,
        ));
        _saveProfiles();
      });
      ScaffoldMessenger.of(context).showSnackBar(const SnackBar(content: Text('Imported successfully')));
    } catch (e) {
      ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text('Error: $e')));
    }
  }

  void _showAddProfileMenu() {
    showModalBottomSheet(
      context: context,
      backgroundColor: Theme.of(context).colorScheme.surface,
      shape: const RoundedRectangleBorder(borderRadius: BorderRadius.vertical(top: Radius.circular(20))),
      builder: (context) => SafeArea(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            ListTile(
              leading: const Icon(Icons.qr_code_scanner, color: Colors.white),
              title: const Text('Import from QR code'),
              onTap: () async {
                Navigator.pop(context);
                final result = await Navigator.push(
                  context,
                  MaterialPageRoute(builder: (context) => const QRScannerScreen()),
                );
                if (result != null && result is String && result.startsWith('ostp://')) {
                  _importFromLink(result);
                }
              },
            ),
            ListTile(
              leading: const Icon(Icons.link, color: Colors.white),
              title: const Text('Import from link'),
              onTap: () {
                Navigator.pop(context);
                _showImportLinkDialog();
              },
            ),
            ListTile(
              leading: const Icon(Icons.edit, color: Colors.white),
              title: const Text('Insert manually'),
              onTap: () {
                Navigator.pop(context);
                _showEditProfileDialog(null);
              },
            ),
          ],
        ),
      ),
    );
  }

  void _showImportLinkDialog() {
    final linkCtrl = TextEditingController();
    showDialog(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Import Link'),
        backgroundColor: Theme.of(context).colorScheme.surface,
        content: TextField(
          controller: linkCtrl,
          decoration: const InputDecoration(hintText: 'ostp://...'),
          autofocus: true,
        ),
        actions: [
          TextButton(onPressed: () => Navigator.pop(context), child: const Text('Cancel')),
          TextButton(
            onPressed: () {
              Navigator.pop(context);
              _importFromLink(linkCtrl.text.trim());
            },
            child: const Text('Import'),
          ),
        ],
      ),
    );
  }

  static const List<String> _stealthDomains = [
    'yastatic.net', 'mc.yandex.ru', 'st.mycdn.me',
    'top-fwz1.mail.ru', 'sso.passport.yandex.ru',
    'sberbank.ru', 'ad.mail.ru', 'ads.vk.com',
    'login.vk.com', 'api.sberbank.ru', 'ok.ru',
    'rostelecom.ru', 'rt.ru', 'tinkoff.ru',
    'x5.ru', 'ozon.ru', 'wildberries.ru', 'gosuslugi.ru', 'vk.com',
  ];

  void _showEditProfileDialog(OstpProfile? profile) {
    final isNew = profile == null;
    final nameCtrl = TextEditingController(text: profile?.name ?? '');
    final serverCtrl = TextEditingController(text: profile?.serverAddr ?? '');
    final keyCtrl = TextEditingController(text: profile?.accessKey ?? '');
    final fragChunkCtrl = TextEditingController(text: (profile?.fragChunk ?? 2).toString());
    final fragSleepCtrl = TextEditingController(text: (profile?.fragSleep ?? 2).toString());
    final junkPcMinCtrl = TextEditingController(text: (profile?.junkPcMin ?? 2).toString());
    final junkPcMaxCtrl = TextEditingController(text: (profile?.junkPcMax ?? 5).toString());
    final junkPsMinCtrl = TextEditingController(text: (profile?.junkPsMin ?? 100).toString());
    final junkPsMaxCtrl = TextEditingController(text: (profile?.junkPsMax ?? 1000).toString());
    String transportMode = profile?.transportMode ?? 'udp';
    bool tcpFragmentation = profile?.tcpFragmentation ?? false;
    String stealthSni = (profile?.stealthSni.isNotEmpty ?? false) ? profile!.stealthSni : 'vk.com';
    bool obscureKey = true;

    showDialog(
      context: context,
      builder: (context) {
        return StatefulBuilder(
          builder: (context, setDialogState) => AlertDialog(
            title: Text(isNew ? 'New Profile' : 'Edit Profile'),
            backgroundColor: Theme.of(context).colorScheme.surface,
            content: SingleChildScrollView(
              child: Column(
                mainAxisSize: MainAxisSize.min,
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  TextField(controller: nameCtrl, decoration: const InputDecoration(labelText: 'Name')),
                  const SizedBox(height: 12),
                  TextField(controller: serverCtrl, decoration: const InputDecoration(labelText: 'Server Address (host:port)')),
                  const SizedBox(height: 12),
                  TextField(
                    controller: keyCtrl,
                    obscureText: obscureKey,
                    decoration: InputDecoration(
                      labelText: 'Access Key',
                      suffixIcon: IconButton(
                        icon: Icon(obscureKey ? Icons.visibility : Icons.visibility_off, size: 18),
                        onPressed: () => setDialogState(() => obscureKey = !obscureKey),
                      ),
                    ),
                  ),
                  const SizedBox(height: 16),
                  DropdownButtonFormField<String>(
                    value: transportMode,
                    decoration: const InputDecoration(labelText: 'Transport'),
                    items: const [
                      DropdownMenuItem(value: 'udp', child: Text('UDP')),
                      DropdownMenuItem(value: 'uot', child: Text('TCP (UoT) — xHTTP stealth')),
                    ],
                    onChanged: (v) {
                      if (v != null) setDialogState(() => transportMode = v);
                    },
                  ),
                  if (transportMode == 'uot') ...[
                    const SizedBox(height: 12),
                    Builder(builder: (context) {
                      final domains = [..._stealthDomains];
                      if (!domains.contains(stealthSni)) domains.add(stealthSni);
                      return DropdownButtonFormField<String>(
                        value: stealthSni,
                        decoration: const InputDecoration(labelText: 'Stealth SNI domain'),
                        items: domains.map((d) => DropdownMenuItem(value: d, child: Text(d))).toList(),
                        onChanged: (v) {
                          if (v != null) setDialogState(() => stealthSni = v);
                        },
                      );
                    }),
                  ],
                  const Divider(height: 32),
                  // ── Junk packets + TCP fragmentation — same per-profile
                  // fields/defaults as the desktop GUI's profile editor. ──
                  const Text('DPI obfuscation', style: TextStyle(fontWeight: FontWeight.bold, fontSize: 13, color: Colors.white54, letterSpacing: 1.0)),
                  const SizedBox(height: 12),
                  Row(
                    children: [
                      Expanded(child: TextField(controller: junkPcMinCtrl, keyboardType: TextInputType.number, decoration: const InputDecoration(labelText: 'Junk packets (min)'))),
                      const SizedBox(width: 12),
                      Expanded(child: TextField(controller: junkPcMaxCtrl, keyboardType: TextInputType.number, decoration: const InputDecoration(labelText: 'Junk packets (max)'))),
                    ],
                  ),
                  const SizedBox(height: 12),
                  Row(
                    children: [
                      Expanded(child: TextField(controller: junkPsMinCtrl, keyboardType: TextInputType.number, decoration: const InputDecoration(labelText: 'Junk size (min, bytes)'))),
                      const SizedBox(width: 12),
                      Expanded(child: TextField(controller: junkPsMaxCtrl, keyboardType: TextInputType.number, decoration: const InputDecoration(labelText: 'Junk size (max, bytes)'))),
                    ],
                  ),
                  const SizedBox(height: 8),
                  SwitchListTile(
                    contentPadding: EdgeInsets.zero,
                    title: const Text('TCP Fragmentation', style: TextStyle(fontSize: 14)),
                    subtitle: const Text('Split the handshake into small chunks', style: TextStyle(fontSize: 12, color: Colors.white54)),
                    value: tcpFragmentation,
                    onChanged: (v) => setDialogState(() => tcpFragmentation = v),
                  ),
                  if (tcpFragmentation) ...[
                    const SizedBox(height: 4),
                    Row(
                      children: [
                        Expanded(child: TextField(controller: fragChunkCtrl, keyboardType: TextInputType.number, decoration: const InputDecoration(labelText: 'Chunk size (bytes)'))),
                        const SizedBox(width: 12),
                        Expanded(child: TextField(controller: fragSleepCtrl, keyboardType: TextInputType.number, decoration: const InputDecoration(labelText: 'Delay (ms)'))),
                      ],
                    ),
                  ],
                ],
              ),
            ),
            actions: [
              if (!isNew)
                TextButton(
                  onPressed: () {
                    setState(() {
                      final wasActive = profile.active;
                      _profiles.removeWhere((p) => p.id == profile.id);
                      if (wasActive && _profiles.isNotEmpty) {
                        _profiles.first.active = true;
                      }
                      _saveProfiles();
                    });
                    Navigator.pop(context);
                  },
                  child: const Text('Delete', style: TextStyle(color: Colors.redAccent)),
                ),
              TextButton(onPressed: () => Navigator.pop(context), child: const Text('Cancel')),
              TextButton(
                onPressed: () {
                  final server = serverCtrl.text.trim();
                  final key = keyCtrl.text.trim();
                  if (server.isEmpty || key.isEmpty) {
                    ScaffoldMessenger.of(context).showSnackBar(
                      const SnackBar(content: Text('Server and Access Key are required')),
                    );
                    return;
                  }
                  setState(() {
                    if (isNew) {
                      final wasEmpty = _profiles.isEmpty;
                      _profiles.add(OstpProfile(
                        id: DateTime.now().millisecondsSinceEpoch.toString(),
                        name: nameCtrl.text.trim().isNotEmpty ? nameCtrl.text.trim() : server,
                        serverAddr: server,
                        accessKey: key,
                        transportMode: transportMode,
                        stealthSni: stealthSni,
                        active: wasEmpty,
                        tcpFragmentation: tcpFragmentation,
                        fragChunk: int.tryParse(fragChunkCtrl.text) ?? 2,
                        fragSleep: int.tryParse(fragSleepCtrl.text) ?? 2,
                        junkPcMin: int.tryParse(junkPcMinCtrl.text) ?? 2,
                        junkPcMax: int.tryParse(junkPcMaxCtrl.text) ?? 5,
                        junkPsMin: int.tryParse(junkPsMinCtrl.text) ?? 100,
                        junkPsMax: int.tryParse(junkPsMaxCtrl.text) ?? 1000,
                      ));
                    } else {
                      profile.name = nameCtrl.text.trim().isNotEmpty ? nameCtrl.text.trim() : server;
                      profile.serverAddr = server;
                      profile.accessKey = key;
                      profile.transportMode = transportMode;
                      profile.stealthSni = stealthSni;
                      profile.tcpFragmentation = tcpFragmentation;
                      profile.fragChunk = int.tryParse(fragChunkCtrl.text) ?? 2;
                      profile.fragSleep = int.tryParse(fragSleepCtrl.text) ?? 2;
                      profile.junkPcMin = int.tryParse(junkPcMinCtrl.text) ?? 2;
                      profile.junkPcMax = int.tryParse(junkPcMaxCtrl.text) ?? 5;
                      profile.junkPsMin = int.tryParse(junkPsMinCtrl.text) ?? 100;
                      profile.junkPsMax = int.tryParse(junkPsMaxCtrl.text) ?? 1000;
                    }
                    _saveProfiles();
                  });
                  Navigator.pop(context);
                },
                child: const Text('Save'),
              ),
            ],
          ),
        );
      },
    );
  }

  void _showShareModal(OstpProfile p) {
    final key = Uri.encodeComponent(p.accessKey);
    if (p.serverAddr.isEmpty || p.accessKey.isEmpty) return;
    final queryParams = <String>[];
    if (p.stealthSni.isNotEmpty) queryParams.add('sni=${Uri.encodeComponent(p.stealthSni)}');
    if (p.transportMode != 'udp') queryParams.add('type=${p.transportMode}');
    final queryString = queryParams.isEmpty ? '' : '?${queryParams.join('&')}';
    final url = 'ostp://$key@${p.serverAddr}$queryString';

    showDialog(
      context: context,
      builder: (context) => AlertDialog(
        backgroundColor: Theme.of(context).colorScheme.surface,
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(20)),
        title: Text('Share "${p.name}"', textAlign: TextAlign.center),
        content: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Container(
              padding: const EdgeInsets.all(16),
              decoration: BoxDecoration(color: Colors.white, borderRadius: BorderRadius.circular(16)),
              child: QrImageView(data: url, version: QrVersions.auto, size: 200.0),
            ),
            const SizedBox(height: 20),
            ElevatedButton.icon(
              onPressed: () {
                Clipboard.setData(ClipboardData(text: url));
                ScaffoldMessenger.of(context).showSnackBar(const SnackBar(content: Text('Copied to clipboard')));
                Navigator.pop(context);
              },
              icon: const Icon(Icons.copy_rounded, color: Colors.white),
              label: const Text('Copy Link', style: TextStyle(color: Colors.white)),
              style: ElevatedButton.styleFrom(
                backgroundColor: Theme.of(context).colorScheme.primary,
                padding: const EdgeInsets.symmetric(horizontal: 24, vertical: 12),
                shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(12)),
              ),
            ),
          ],
        ),
        actions: [TextButton(onPressed: () => Navigator.pop(context), child: const Text('Close'))],
      ),
    );
  }

  // ── Widgets ──────────────────────────────────────────────────────────────

  Widget _buildTextField(String label, TextEditingController controller, {String? hint, int maxLines = 1}) {
    return Padding(
      padding: const EdgeInsets.only(bottom: 24),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(label, style: const TextStyle(color: Colors.white54, fontSize: 13, fontWeight: FontWeight.bold, letterSpacing: 1.0)),
          const SizedBox(height: 10),
          TextField(
            controller: controller,
            maxLines: maxLines,
            style: const TextStyle(fontSize: 16),
            decoration: InputDecoration(
              hintText: hint,
              hintStyle: const TextStyle(color: Colors.white30),
              filled: true,
              fillColor: Theme.of(context).colorScheme.surface,
              border: OutlineInputBorder(borderRadius: BorderRadius.circular(12), borderSide: BorderSide.none),
              contentPadding: const EdgeInsets.symmetric(horizontal: 16, vertical: 16),
            ),
          ),
        ],
      ),
    );
  }

  Widget _buildToggle(String title, String subtitle, bool value, ValueChanged<bool> onChanged) {
    return Padding(
      padding: const EdgeInsets.only(bottom: 24),
      child: Row(
        mainAxisAlignment: MainAxisAlignment.spaceBetween,
        children: [
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(title, style: const TextStyle(fontSize: 16, fontWeight: FontWeight.bold)),
                const SizedBox(height: 4),
                Text(subtitle, style: const TextStyle(fontSize: 13, color: Colors.white54)),
              ],
            ),
          ),
          Switch(
            value: value,
            onChanged: (v) {
              setState(() => onChanged(v));
              _saveSettings();
            },
            activeColor: Theme.of(context).colorScheme.secondary,
          )
        ],
      ),
    );
  }

  List<Widget> _buildProfileCards() {
    String? activeId;
    for (final x in _profiles) {
      if (x.active) { activeId = x.id; break; }
    }
    return _profiles.map((p) => Card(
      color: p.active
          ? Theme.of(context).colorScheme.primary.withOpacity(0.12)
          : Theme.of(context).colorScheme.surface,
      margin: const EdgeInsets.only(bottom: 12),
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(16),
        side: p.active ? BorderSide(color: Theme.of(context).colorScheme.primary.withOpacity(0.4)) : BorderSide.none,
      ),
      child: ListTile(
        leading: Radio<String>(
          value: p.id,
          groupValue: activeId,
          onChanged: (_) => _selectActive(p),
        ),
        title: Text(p.name, style: const TextStyle(fontWeight: FontWeight.bold)),
        subtitle: Text('${p.serverAddr} (${p.transportMode.toUpperCase()})', style: const TextStyle(fontSize: 12)),
        trailing: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            IconButton(
              icon: const Icon(Icons.qr_code_rounded, size: 20, color: Colors.white54),
              onPressed: () => _showShareModal(p),
            ),
            IconButton(
              icon: const Icon(Icons.edit, size: 20, color: Colors.white54),
              onPressed: () => _showEditProfileDialog(p),
            ),
          ],
        ),
        onTap: () => _selectActive(p),
      ),
    )).toList();
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: const Text('Configuration', style: TextStyle(fontWeight: FontWeight.bold)),
        backgroundColor: Colors.transparent,
        elevation: 0,
        leading: IconButton(
          icon: const Icon(Icons.arrow_back_rounded),
          onPressed: () => Navigator.pop(context),
        ),
        actions: [
          IconButton(
            icon: const Icon(Icons.add_rounded),
            tooltip: 'Add Profile',
            onPressed: _showAddProfileMenu,
          ),
        ],
      ),
      body: Stack(
        children: [
          Positioned.fill(
            child: Opacity(
              opacity: 0.1,
              child: Center(
                child: Image.asset(
                  'assets/logo.png',
                  width: MediaQuery.of(context).size.shortestSide * 0.6,
                  color: Colors.white,
                ),
              ),
            ),
          ),
          ListView(
            padding: const EdgeInsets.symmetric(horizontal: 24, vertical: 16),
            children: [
              const Text('PROFILES', style: TextStyle(color: Colors.white54, fontSize: 13, fontWeight: FontWeight.bold, letterSpacing: 1.0)),
              const SizedBox(height: 16),
              if (_profiles.isEmpty)
                Center(
                  child: Padding(
                    padding: const EdgeInsets.all(32.0),
                    child: Text('Create a new profile', style: TextStyle(color: Colors.white54, fontSize: 18)),
                  ),
                )
              else
                ..._buildProfileCards(),

          const SizedBox(height: 32),
          const Text('CLIENT SETTINGS', style: TextStyle(color: Colors.white54, fontSize: 13, fontWeight: FontWeight.bold, letterSpacing: 1.0)),
          const SizedBox(height: 16),

          Container(
            padding: const EdgeInsets.all(24),
            decoration: BoxDecoration(
              color: Colors.white.withOpacity(0.02),
              borderRadius: BorderRadius.circular(24),
              border: Border.all(color: Colors.white.withOpacity(0.05)),
            ),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                _buildToggle('MUX (Multiplexing)', 'Multiple sessions over single connection', _muxEnabled, (v) => _muxEnabled = v),
                if (_muxEnabled)
                  _buildTextField('MUX Sessions', _muxSessionsCtrl, hint: 'e.g. 2, 4, 8'),

                _buildToggle('Debug Mode', 'Verbose logging', _debugMode, (v) => _debugMode = v),

                _buildTextField('Local Proxy Bind', _localBindCtrl, hint: '127.0.0.1:1088'),
                _buildTextField('Custom DNS Server', _dnsCtrl, hint: '1.1.1.1 (e.g. 8.8.8.8)'),
                _buildTextField('MTU (Packet Size)', _mtuCtrl, hint: '1140 (decrease if connection drops)'),

                const Padding(
                  padding: EdgeInsets.only(bottom: 16),
                  child: Row(
                    children: [
                      Text('Exclusions', style: TextStyle(fontSize: 18, fontWeight: FontWeight.bold)),
                      SizedBox(width: 10),
                      Text('one per line', style: TextStyle(fontSize: 13, color: Colors.white30)),
                    ],
                  ),
                ),
                _buildTextField('Bypass Domains', _domainsCtrl, hint: 'example.com\n*.google.com', maxLines: 3),
                _buildTextField('Bypass IPs / CIDR', _ipsCtrl, hint: '192.168.1.0/24\n10.0.0.1', maxLines: 3),
                _buildTextField('Bypass Processes', _processesCtrl, hint: 'com.example.app', maxLines: 3),

                const SizedBox(height: 8),
                SizedBox(
                  width: double.infinity,
                  child: ElevatedButton.icon(
                    icon: const Icon(Icons.route),
                    label: const Text('Configure Split Tunneling'),
                    onPressed: () {
                      Navigator.push(context, MaterialPageRoute(builder: (context) => AppRoutingScreen(prefs: widget.prefs)));
                    },
                  ),
                ),
                const SizedBox(height: 16),
                SizedBox(
                  width: double.infinity,
                  child: ElevatedButton.icon(
                    icon: const Icon(Icons.article),
                    label: const Text('View Logs'),
                    onPressed: () {
                      Navigator.push(context, MaterialPageRoute(builder: (context) => const LogsScreen()));
                    },
                  ),
                ),
              ],
            ),
          ),

          const SizedBox(height: 16),

          InkWell(
            onTap: _isCheckingUpdates ? null : _checkForUpdates,
            child: Container(
              padding: const EdgeInsets.symmetric(horizontal: 20, vertical: 16),
              decoration: BoxDecoration(
                color: Colors.white.withOpacity(0.02),
                borderRadius: BorderRadius.circular(16),
                border: Border.all(color: Colors.white.withOpacity(0.05)),
              ),
              child: Row(
                children: [
                  const Icon(Icons.system_update_rounded, color: Colors.white70, size: 24),
                  const SizedBox(width: 16),
                  Expanded(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        const Text('Check for Updates', style: TextStyle(fontWeight: FontWeight.bold, fontSize: 16, color: Colors.white)),
                        const SizedBox(height: 4),
                        Text(
                          _isCheckingUpdates ? 'Checking...' : 'Check latest release on GitHub',
                          style: const TextStyle(fontSize: 13, color: Colors.white54),
                        ),
                      ],
                    ),
                  ),
                  if (_isCheckingUpdates)
                    const SizedBox(width: 16, height: 16, child: CircularProgressIndicator(strokeWidth: 2, color: Colors.white54))
                  else
                    const Icon(Icons.arrow_forward_ios_rounded, color: Colors.white54, size: 16),
                ],
              ),
            ),
          ),

          const SizedBox(height: 40),
        ],
      ),
      ],
      ),
    );
  }

  Future<void> _checkForUpdates() async {
    if (_isCheckingUpdates) return;
    setState(() { _isCheckingUpdates = true; });
    try {
      final packageInfo = await PackageInfo.fromPlatform();
      final currentVersion = packageInfo.version;

      final response = await http.get(Uri.parse('https://api.github.com/repos/ospab/ostp/releases/latest'));
      if (response.statusCode == 200) {
        final data = json.decode(response.body);
        final latestVersion = (data['tag_name'] as String).replaceAll('v', '');
        final hasUpdate = latestVersion != currentVersion;

        if (!mounted) return;
        showDialog(
          context: context,
          builder: (context) {
            return AlertDialog(
              backgroundColor: Theme.of(context).colorScheme.surface,
              title: Text(hasUpdate ? 'Update Available!' : 'Up to Date'),
              content: Text(hasUpdate
                  ? 'A new version ($latestVersion) is available on GitHub. You are currently running version $currentVersion.'
                  : 'You are running the latest version ($currentVersion).'),
              actions: [
                TextButton(onPressed: () => Navigator.pop(context), child: const Text('Close')),
                if (hasUpdate)
                  TextButton(
                    onPressed: () {
                      Navigator.pop(context);
                      final url = Uri.parse(data['html_url'] ?? 'https://github.com/ospab/ostp/releases/latest');
                      launchUrl(url, mode: LaunchMode.externalApplication);
                    },
                    child: const Text('Download'),
                  )
              ],
            );
          },
        );
      } else {
        throw Exception('HTTP ${response.statusCode}');
      }
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text('Error checking updates: $e')));
    } finally {
      if (mounted) setState(() { _isCheckingUpdates = false; });
    }
  }
}
