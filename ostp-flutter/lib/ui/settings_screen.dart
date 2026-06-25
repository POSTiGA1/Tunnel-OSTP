import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:ui';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:mobile_scanner/mobile_scanner.dart';
import 'app_routing_screen.dart';
import 'logs_screen.dart';
import 'qr_scanner_screen.dart';
import 'package:qr_flutter/qr_flutter.dart';
import 'package:http/http.dart' as http;
import 'package:url_launcher/url_launcher.dart';
import 'package:package_info_plus/package_info_plus.dart';

class SettingsScreen extends StatefulWidget {
  final SharedPreferences prefs;
  const SettingsScreen({super.key, required this.prefs});

  @override
  State<SettingsScreen> createState() => _SettingsScreenState();
}

class _SettingsScreenState extends State<SettingsScreen> {
  late TextEditingController _importCtrl;
  late TextEditingController _serverCtrl;
  late TextEditingController _localBindCtrl;
  late TextEditingController _keyCtrl;
  late TextEditingController _dnsCtrl;
  late TextEditingController _mtuCtrl;
  late TextEditingController _domainsCtrl;
  late TextEditingController _ipsCtrl;
  late TextEditingController _processesCtrl;
  late TextEditingController _dnsDomainCtrl;
  late TextEditingController _pbkCtrl;
  late TextEditingController _sidCtrl;

  bool _obscureKey = true;
  bool _debugMode = false;
  late TextEditingController _dnsRegionCtrl;
  String _transportMode = 'udp'; // 'udp' | 'uot'
  String _tunStack = 'ostp'; // 'system' | 'ostp'
  bool _muxEnabled = false;
  late TextEditingController _muxSessionsCtrl;
  bool _isCheckingUpdates = false;

  bool _tcpFragmentation = false;

  @override
  void initState() {
    super.initState();
    _importCtrl = TextEditingController();
    _loadSettings();
  }

  void _loadSettings() {
    _serverCtrl = TextEditingController(text: widget.prefs.getString('server_addr') ?? '');
    _localBindCtrl = TextEditingController(text: widget.prefs.getString('local_bind') ?? '127.0.0.1:1088');
    _keyCtrl = TextEditingController(text: widget.prefs.getString('access_key') ?? '');
    _dnsCtrl = TextEditingController(text: widget.prefs.getString('dns_server') ?? '');
    _mtuCtrl = TextEditingController(text: widget.prefs.getString('mtu') ?? '1140');
    _transportMode = widget.prefs.getString('transport_mode') ?? 'udp';
    _tcpFragmentation = widget.prefs.getBool('tcp_fragmentation') ?? false;
    _domainsCtrl = TextEditingController(text: widget.prefs.getString('ex_domains') ?? '');
    _ipsCtrl = TextEditingController(text: widget.prefs.getString('ex_ips') ?? '');
    _processesCtrl = TextEditingController(text: widget.prefs.getString('ex_processes') ?? '');
    _dnsDomainCtrl = TextEditingController(text: widget.prefs.getString('dns_domain') ?? '');
    _dnsRegionCtrl = TextEditingController(text: widget.prefs.getString('dns_region') ?? '1.1.1.1');
    _pbkCtrl = TextEditingController(text: widget.prefs.getString('tun_pbk') ?? '');
    _sidCtrl = TextEditingController(text: widget.prefs.getString('sid') ?? '');
    _tunStack = widget.prefs.getString('tun_stack') ?? 'ostp';
    _debugMode = widget.prefs.getBool('debug_mode') ?? false;
    _muxEnabled = widget.prefs.getBool('mux_enabled') ?? false;
    _muxSessionsCtrl = TextEditingController(text: widget.prefs.getString('mux_sessions') ?? '2');
  }

  @override
  void dispose() {
    _saveSettings();
    _importCtrl.dispose();
    _serverCtrl.dispose();
    _localBindCtrl.dispose();
    _keyCtrl.dispose();
    _dnsCtrl.dispose();
    _mtuCtrl.dispose();
    _domainsCtrl.dispose();
    _ipsCtrl.dispose();
    _processesCtrl.dispose();
    _dnsDomainCtrl.dispose();
    _dnsRegionCtrl.dispose();
    _pbkCtrl.dispose();
    _sidCtrl.dispose();
    _muxSessionsCtrl.dispose();
    super.dispose();
  }

  void _saveSettings() {
    widget.prefs.setString('server_addr', _serverCtrl.text.trim());
    widget.prefs.setString('local_bind', _localBindCtrl.text.trim());
    widget.prefs.setString('access_key', _keyCtrl.text.trim());
    widget.prefs.setString('dns_server', _dnsCtrl.text.trim());
    widget.prefs.setString('mtu', _mtuCtrl.text.trim());
    widget.prefs.setString('transport_mode', _transportMode);
    widget.prefs.setBool('tcp_fragmentation', _tcpFragmentation);
    widget.prefs.setString('ex_domains', _domainsCtrl.text.trim());
    widget.prefs.setString('ex_ips', _ipsCtrl.text.trim());
    widget.prefs.setString('ex_processes', _processesCtrl.text.trim());
    widget.prefs.setBool('debug_mode', _debugMode);
    widget.prefs.setString('tun_stack', _tunStack);
    widget.prefs.setString('dns_domain', _dnsDomainCtrl.text.trim());
    widget.prefs.setString('dns_region', _dnsRegionCtrl.text.trim());
    widget.prefs.setString('tun_pbk', _pbkCtrl.text.trim());
    widget.prefs.setString('sid', _sidCtrl.text.trim());
    widget.prefs.setBool('mux_enabled', _muxEnabled);
    widget.prefs.setString('mux_sessions', _muxSessionsCtrl.text.trim());
  }
  Widget _buildTextField(String label, TextEditingController controller, {String? hint, bool isPassword = false, int maxLines = 1, bool isMono = false}) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(label, style: const TextStyle(color: Colors.white54, fontSize: 13, fontWeight: FontWeight.bold, letterSpacing: 1.0)),
        const SizedBox(height: 10),
        TextField(
          controller: controller,
          obscureText: isPassword && _obscureKey,
          maxLines: maxLines,
          style: TextStyle(fontSize: 16, fontFamily: isMono ? 'monospace' : 'Inter'),
          decoration: InputDecoration(
            hintText: hint,
            hintStyle: const TextStyle(color: Colors.white30),
            filled: true,
            fillColor: Theme.of(context).colorScheme.surface,
            border: OutlineInputBorder(borderRadius: BorderRadius.circular(12), borderSide: BorderSide.none),
            contentPadding: const EdgeInsets.symmetric(horizontal: 16, vertical: 16),
            suffixIcon: isPassword ? IconButton(
              icon: Icon(_obscureKey ? Icons.visibility : Icons.visibility_off, color: Colors.white54),
              onPressed: () => setState(() => _obscureKey = !_obscureKey),
            ) : null,
          ),
        ),
        const SizedBox(height: 24),
      ],
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
              onChanged(v);
              _saveSettings();
            },
            activeColor: Theme.of(context).colorScheme.secondary,
            activeTrackColor: Theme.of(context).colorScheme.secondary.withOpacity(0.3),
            inactiveTrackColor: Colors.white10,
          )
        ],
      ),
    );
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
            icon: const Icon(Icons.share_rounded),
            tooltip: 'Share Config',
            onPressed: _showShareModal,
          ),
          IconButton(
            icon: const Icon(Icons.qr_code_scanner_rounded),
            onPressed: () async {
              final result = await Navigator.push(
                context,
                MaterialPageRoute(builder: (context) => const QRScannerScreen()),
              );
              if (result != null && result is String && result.startsWith('ostp://')) {
                setState(() {
                  _importCtrl.text = result;
                });
              }
            },
          )
        ],
      ),
      body: ListView(
        padding: const EdgeInsets.symmetric(horizontal: 24, vertical: 16),
        children: [
          // Quick Import Row
          Row(
            children: [
              Expanded(
                child: TextField(
                  controller: _importCtrl,
                  decoration: InputDecoration(
                    hintText: 'Paste ostp:// share link...',
                    hintStyle: const TextStyle(color: Colors.white30, fontSize: 14),
                    filled: true,
                    fillColor: Colors.white.withOpacity(0.05),
                    border: OutlineInputBorder(borderRadius: BorderRadius.circular(20), borderSide: BorderSide.none),
                    contentPadding: const EdgeInsets.symmetric(horizontal: 16, vertical: 14),
                  ),
                ),
              ),
              const SizedBox(width: 12),
              ElevatedButton(
                onPressed: () {
                  final raw = _importCtrl.text.trim();
                  if (raw.isEmpty) return;
                  try {
                    if (!raw.startsWith('ostp://')) {
                      throw Exception('Link must start with ostp://');
                    }
                    final uri = Uri.parse(raw);
                    final key = Uri.decodeComponent(uri.userInfo);
                    final host = uri.authority.replaceFirst(uri.userInfo + '@', '');
                    if (key.isEmpty || host.isEmpty) {
                      throw Exception('Incomplete link parameters');
                    }
                    setState(() {
                      _serverCtrl.text = host;
                      _keyCtrl.text = key;
                      _dnsDomainCtrl.text = uri.queryParameters['domain'] ?? '';
                      _dnsRegionCtrl.text = uri.queryParameters['resolver'] ?? '1.1.1.1';
                      
                      final type = uri.queryParameters['type'];
                      _transportMode = type == 'tcp' || type == 'http' ? 'uot' : (type == 'dns' ? 'dns' : 'udp');
                      _importCtrl.clear();

                      _saveSettings();
                    });
                    ScaffoldMessenger.of(context).showSnackBar(const SnackBar(content: Text('Imported successfully')));
                  } catch (e) {
                    ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text('Error: ${e.toString()}')));
                  }
                },
                style: ElevatedButton.styleFrom(
                  padding: const EdgeInsets.symmetric(horizontal: 20, vertical: 14),
                  backgroundColor: Colors.white,
                  foregroundColor: Colors.black,
                  shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(20)),
                ),
                child: const Text('Import', style: TextStyle(fontWeight: FontWeight.bold, color: Colors.black)),
              )
            ],
          ),
          
          const SizedBox(height: 30),
          
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
                _buildTextField('Server Address', _serverCtrl, hint: 'host:port'),
                _buildTextField('Local Proxy Bind', _localBindCtrl, hint: '127.0.0.1:1088'),
                _buildTextField('Access Key', _keyCtrl, hint: 'Secure access key', isPassword: true),
                _buildTextField('Custom DNS Server', _dnsCtrl, hint: '1.1.1.1 (e.g. 8.8.8.8)'),
                _buildTextField('MTU (Packet Size)', _mtuCtrl, hint: '1140 (decrease if connection drops)'),

                // ── Transport Mode ───────────────────────────────────────
                const Text('Transport Mode', style: TextStyle(color: Colors.white54, fontSize: 13, fontWeight: FontWeight.bold, letterSpacing: 1.0)),
                const SizedBox(height: 10),
                Container(
                  decoration: BoxDecoration(
                    color: Theme.of(context).colorScheme.surface,
                    borderRadius: BorderRadius.circular(12),
                  ),
                  child: Column(
                    children: [
                      RadioListTile<String>(
                        value: 'udp',
                        groupValue: _transportMode,
                        title: const Text('UDP (Default)', style: TextStyle(fontWeight: FontWeight.w600)),
                        subtitle: const Text('Fast, works on Wi-Fi and most networks', style: TextStyle(color: Colors.white54, fontSize: 12)),
                        activeColor: Theme.of(context).colorScheme.secondary,
                        onChanged: (v) => setState(() { _transportMode = v!; _saveSettings(); }),
                      ),
                      Divider(color: Colors.white.withOpacity(0.05), height: 1),
                      RadioListTile<String>(
                        value: 'uot',
                        groupValue: _transportMode,
                        title: const Text('UoT (UDP-over-TCP)', style: TextStyle(fontWeight: FontWeight.w600)),
                        subtitle: const Text('Reliable on strict networks. Enables TCP DPI bypass.', style: TextStyle(color: Colors.white54, fontSize: 12)),
                        activeColor: Theme.of(context).colorScheme.primary,
                        onChanged: (v) => setState(() { _transportMode = v!; _saveSettings(); }),
                      ),
                      if (_transportMode == 'uot')
                        Padding(
                          padding: const EdgeInsets.only(left: 16.0, right: 8.0, bottom: 8.0),
                          child: SwitchListTile(
                            title: const Text('TCP Fragmentation', style: TextStyle(fontSize: 14, fontWeight: FontWeight.w500)),
                            subtitle: const Text('Bypass DPI by chunking handshake (Zapret style)', style: TextStyle(fontSize: 12, color: Colors.white54)),
                            value: _tcpFragmentation,
                            activeColor: Theme.of(context).colorScheme.primary,
                            onChanged: (v) => setState(() { _tcpFragmentation = v; _saveSettings(); }),
                          ),
                        ),
                      Divider(color: Colors.white.withOpacity(0.05), height: 1),
                      RadioListTile<String>(
                        value: 'dns',
                        groupValue: _transportMode,
                        title: const Text('DNS Tunnel', style: TextStyle(fontWeight: FontWeight.w600)),
                        subtitle: const Text('Very slow, but works under strict DPI blocks', style: TextStyle(color: Colors.orangeAccent, fontSize: 12)),
                        activeColor: Colors.orangeAccent,
                        onChanged: (v) => setState(() { _transportMode = v!; _saveSettings(); }),
                      ),
                    ],
                  ),
                ),
                
                const SizedBox(height: 16),
                
                // DNS Proxy parameters
                AnimatedCrossFade(
                  duration: const Duration(milliseconds: 250),
                  crossFadeState: _transportMode == 'dns' ? CrossFadeState.showFirst : CrossFadeState.showSecond,
                  firstChild: Container(
                    padding: const EdgeInsets.all(16),
                    decoration: BoxDecoration(
                      color: Colors.orangeAccent.withOpacity(0.06),
                      borderRadius: BorderRadius.circular(16),
                      border: Border.all(color: Colors.orangeAccent.withOpacity(0.2)),
                    ),
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Row(
                          children: [
                            const Icon(Icons.dns, size: 16, color: Colors.orangeAccent),
                            const SizedBox(width: 8),
                            const Text('DNS Tunnel Settings', style: TextStyle(fontWeight: FontWeight.bold, color: Colors.orangeAccent, fontSize: 14)),
                          ],
                        ),
                        const SizedBox(height: 4),
                        const Text(
                          'Specify the domain pointing to your server. Details in Wiki.',
                          style: TextStyle(fontSize: 12, color: Colors.white38),
                        ),
                        const SizedBox(height: 16),
                        _buildTextField('Domain (Points to Server)', _dnsDomainCtrl, hint: 'tunnel.myvpn.com'),
                        const SizedBox(height: 16),
                        Row(
                          children: [
                            Expanded(
                              child: _buildTextField('DNS Resolver Server', _dnsRegionCtrl, hint: '1.1.1.1'),
                            ),
                            const SizedBox(width: 8),
                            Padding(
                              padding: const EdgeInsets.only(top: 24.0),
                              child: ElevatedButton(
                                onPressed: _showDnsProberDialog,
                                style: ElevatedButton.styleFrom(
                                  backgroundColor: Colors.orangeAccent.withOpacity(0.2),
                                  foregroundColor: Colors.orangeAccent,
                                  elevation: 0,
                                  padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 14),
                                  shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(12)),
                                ),
                                child: const Text('PROBER', style: TextStyle(fontWeight: FontWeight.bold, fontSize: 12)),
                              ),
                            )
                          ],
                        ),
                      ],
                    ),
                  ),
                  secondChild: const SizedBox.shrink(),
                ),

                const SizedBox(height: 16),
                _buildToggle('Multiplexing (Mux)', 'Combine multiple TCP streams to bypass throttling', _muxEnabled, (v) => setState(() => _muxEnabled = v)),
                AnimatedCrossFade(
                  duration: const Duration(milliseconds: 200),
                  crossFadeState: _muxEnabled ? CrossFadeState.showFirst : CrossFadeState.showSecond,
                  firstChild: Padding(
                    padding: const EdgeInsets.only(top: 12.0),
                    child: _buildTextField('Mux Sessions', _muxSessionsCtrl, hint: '4'),
                  ),
                  secondChild: const SizedBox.shrink(),
                ),

                Row(
                  mainAxisAlignment: MainAxisAlignment.spaceBetween,
                  children: [
                    Expanded(child: _buildToggle('Debug Logs', 'Verbose output', _debugMode, (v) => setState(() => _debugMode = v))),
                    Padding(
                      padding: const EdgeInsets.only(bottom: 24.0, left: 10),
                      child: IconButton(
                        icon: const Icon(Icons.receipt_long_rounded),
                        color: Theme.of(context).colorScheme.primary,
                        tooltip: 'View Logs',
                        onPressed: () {
                          Navigator.push(context, MaterialPageRoute(builder: (context) => const LogsScreen()));
                        },
                      ),
                    ),
                  ],
                ),

                
                const Padding(
                  padding: EdgeInsets.symmetric(vertical: 16),
                  child: Row(
                    children: [
                      Text('Exclusions', style: TextStyle(fontSize: 18, fontWeight: FontWeight.bold)),
                      SizedBox(width: 10),
                      Text('one per line', style: TextStyle(fontSize: 13, color: Colors.white30)),
                    ],
                  ),
                ),
                
                _buildTextField('Bypass Domains', _domainsCtrl, hint: 'example.com\n*.google.com', maxLines: 3, isMono: true),
                _buildTextField('Bypass IPs / CIDR', _ipsCtrl, hint: '192.168.1.0/24\n10.0.0.1', maxLines: 3, isMono: true),
                
                // Premium app routing trigger button
                InkWell(
                  onTap: () {
                    Navigator.push(
                      context,
                      MaterialPageRoute(builder: (context) => AppRoutingScreen(prefs: widget.prefs)),
                    );
                  },
                  child: Container(
                    padding: const EdgeInsets.symmetric(horizontal: 20, vertical: 16),
                    decoration: BoxDecoration(
                      color: Theme.of(context).colorScheme.primary.withOpacity(0.08),
                      borderRadius: BorderRadius.circular(16),
                      border: Border.all(color: Theme.of(context).colorScheme.primary.withOpacity(0.2)),
                    ),
                    child: Row(
                      children: [
                        Icon(Icons.apps_rounded, color: Theme.of(context).colorScheme.primary, size: 24),
                        const SizedBox(width: 16),
                        const Expanded(
                          child: Column(
                            crossAxisAlignment: CrossAxisAlignment.start,
                            children: [
                              Text(
                                'Per-App Connection Rules',
                                style: TextStyle(fontWeight: FontWeight.bold, fontSize: 16, color: Colors.white),
                              ),
                              SizedBox(height: 4),
                              Text(
                                'Choose which apps bypass or use VPN',
                                style: TextStyle(fontSize: 13, color: Colors.white54),
                              ),
                            ],
                          ),
                        ),
                        const Icon(Icons.arrow_forward_ios_rounded, color: Colors.white54, size: 16),
                      ],
                    ),
                  ),
                ),
                const SizedBox(height: 10),
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
                  Icon(Icons.system_update_rounded, color: Colors.white70, size: 24),
                  const SizedBox(width: 16),
                  Expanded(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text(
                          'Check for Updates',
                          style: TextStyle(fontWeight: FontWeight.bold, fontSize: 16, color: Colors.white),
                        ),
                        SizedBox(height: 4),
                        Text(
                          _isCheckingUpdates ? 'Checking...' : 'Check latest release on GitHub',
                          style: TextStyle(fontSize: 13, color: Colors.white54),
                        ),
                      ],
                    ),
                  ),
                  if (_isCheckingUpdates)
                    const SizedBox(
                      width: 16,
                      height: 16,
                      child: CircularProgressIndicator(strokeWidth: 2, color: Colors.white54),
                    )
                  else
                    const Icon(Icons.arrow_forward_ios_rounded, color: Colors.white54, size: 16),
                ],
              ),
            ),
          ),
          
          const SizedBox(height: 40),
        ],
      ),
    );
  }

  String _generateShareUrl() {
    final host = _serverCtrl.text.trim();
    final key = Uri.encodeComponent(_keyCtrl.text.trim());
    if (host.isEmpty || key.isEmpty) return '';

    final queryParams = <String>[];
    if (_dnsDomainCtrl.text.trim().isNotEmpty) {
      queryParams.add('domain=${Uri.encodeComponent(_dnsDomainCtrl.text.trim())}');
    }
    final resolver = _dnsRegionCtrl.text.trim();
    if (resolver.isNotEmpty && resolver != '1.1.1.1') {
      queryParams.add('resolver=${Uri.encodeComponent(resolver)}');
    }
    if (_pbkCtrl.text.trim().isNotEmpty) {
      queryParams.add('pbk=${Uri.encodeComponent(_pbkCtrl.text.trim())}');
    }
    if (_sidCtrl.text.trim().isNotEmpty) {
      queryParams.add('sid=${Uri.encodeComponent(_sidCtrl.text.trim())}');
    }
    if (_transportMode != 'udp') {
      queryParams.add('type=$_transportMode');
    }
    
    final queryString = queryParams.isEmpty ? '' : '?${queryParams.join('&')}';
    return 'ostp://$key@$host$queryString';
  }

  void _showShareModal() {
    final url = _generateShareUrl();
    if (url.isEmpty) {
      ScaffoldMessenger.of(context).showSnackBar(const SnackBar(content: Text('Server Address and Access Key are required to share.')));
      return;
    }

    showDialog(
      context: context,
      builder: (context) {
        return AlertDialog(
          backgroundColor: Theme.of(context).colorScheme.surface,
          shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(20)),
          title: const Text('Share Config', textAlign: TextAlign.center),
          content: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              Container(
                padding: const EdgeInsets.all(16),
                decoration: BoxDecoration(
                  color: Colors.white,
                  borderRadius: BorderRadius.circular(16),
                ),
                child: QrImageView(
                  data: url,
                  version: QrVersions.auto,
                  size: 200.0,
                ),
              ),
              const SizedBox(height: 20),
              ElevatedButton.icon(
                onPressed: () {
                  Clipboard.setData(ClipboardData(text: url));
                  ScaffoldMessenger.of(context).showSnackBar(const SnackBar(content: Text('Copied to clipboard')));
                  Navigator.pop(context);
                },
                icon: const Icon(Icons.copy_rounded, color: Colors.black),
                label: const Text('Copy Link', style: TextStyle(color: Colors.black, fontWeight: FontWeight.bold)),
                style: ElevatedButton.styleFrom(
                  backgroundColor: Colors.white,
                  foregroundColor: Colors.black,
                  padding: const EdgeInsets.symmetric(horizontal: 24, vertical: 12),
                  shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(12)),
                ),
              )
            ],
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.pop(context),
              child: const Text('Close'),
            )
          ],
        );
      }
    );
  }

  Future<void> _showDnsProberDialog() async {
    const channel = MethodChannel('com.ospab.ostp/vpn');
    showDialog(
      context: context,
      barrierDismissible: false,
      builder: (context) {
        return StatefulBuilder(
          builder: (context, setModalState) {
            return AlertDialog(
              backgroundColor: Theme.of(context).colorScheme.surface,
              shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(20)),
              title: const Text('DNS Prober', textAlign: TextAlign.center),
              content: FutureBuilder<String?>(
                future: channel.invokeMethod<String>('runDnsProber', {'domain': _dnsDomainCtrl.text.trim()}),
                builder: (context, snapshot) {
                  if (snapshot.connectionState == ConnectionState.waiting) {
                    return const Column(
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        CircularProgressIndicator(),
                        SizedBox(height: 16),
                        Text('Sending real tunnel probes...', style: TextStyle(color: Colors.white54, fontSize: 13), textAlign: TextAlign.center),
                      ],
                    );
                  }

                  if (snapshot.hasError || !snapshot.hasData) {
                    return Text('Error: ${snapshot.error}', style: const TextStyle(color: Colors.redAccent));
                  }

                  List<dynamic> results = [];
                  try {
                    results = jsonDecode(snapshot.data!);
                  } catch (_) {}

                  if (results.isEmpty) {
                    return const Text('No results or all timed out.', style: TextStyle(color: Colors.redAccent));
                  }

                  return SizedBox(
                    width: double.maxFinite,
                    child: ListView.builder(
                      shrinkWrap: true,
                      itemCount: results.length,
                      itemBuilder: (context, index) {
                        final res = results[index];
                        final name = res['name'] ?? '';
                        final ip = res['ip'] ?? '';
                        final latency = res['latency_ms'];
                        
                        final isBest = index == 0 && latency != null;
                        
                        return ListTile(
                          onTap: latency != null ? () {
                            setState(() {
                              _dnsRegionCtrl.text = ip;
                              _saveSettings();
                            });
                            Navigator.pop(context);
                            ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text('DNS set to $ip')));
                          } : null,
                          title: Text('${isBest ? '⭐ ' : ''}$name', style: const TextStyle(fontSize: 14)),
                          subtitle: Text(ip, style: const TextStyle(fontSize: 12, color: Colors.white54)),
                          trailing: Text(
                            latency != null ? '$latency ms' : 'TIMEOUT',
                            style: TextStyle(
                              color: latency == null ? Colors.redAccent : (latency < 100 ? Colors.greenAccent : Colors.orangeAccent),
                              fontWeight: FontWeight.bold,
                            ),
                          ),
                          tileColor: isBest ? Colors.blueAccent.withOpacity(0.1) : null,
                          shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(10)),
                        );
                      },
                    ),
                  );
                },
              ),
              actions: [
                TextButton(
                  onPressed: () => Navigator.pop(context),
                  child: const Text('Close'),
                )
              ],
            );
          }
        );
      }
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
                TextButton(
                  onPressed: () => Navigator.pop(context),
                  child: const Text('Close'),
                ),
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
          }
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

