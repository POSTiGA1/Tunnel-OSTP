import 'dart:convert';

class OstpProfile {
  String id;
  String name;
  String serverAddr;
  String accessKey;
  String transportMode;
  String stealthSni;
  bool wss;
  bool active;

  OstpProfile({
    required this.id,
    required this.name,
    required this.serverAddr,
    required this.accessKey,
    this.transportMode = 'udp',
    this.stealthSni = '',
    this.wss = false,
    this.active = false,
  });

  Map<String, dynamic> toJson() {
    return {
      'id': id,
      'name': name,
      'serverAddr': serverAddr,
      'accessKey': accessKey,
      'transportMode': transportMode,
      'stealthSni': stealthSni,
      'wss': wss,
      'active': active,
    };
  }

  factory OstpProfile.fromJson(Map<String, dynamic> json) {
    return OstpProfile(
      id: json['id'] as String? ?? '',
      name: json['name'] as String? ?? 'Unnamed Profile',
      serverAddr: json['serverAddr'] as String? ?? '',
      accessKey: json['accessKey'] as String? ?? '',
      transportMode: json['transportMode'] as String? ?? 'udp',
      stealthSni: json['stealthSni'] as String? ?? '',
      wss: json['wss'] as bool? ?? false,
      active: json['active'] as bool? ?? false,
    );
  }
}
