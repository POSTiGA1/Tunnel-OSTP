import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:ui';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:mobile_scanner/mobile_scanner.dart';

import 'ui/home_screen.dart';

void main() async {
  WidgetsFlutterBinding.ensureInitialized();
  final prefs = await SharedPreferences.getInstance();
  runApp(OstpApp(prefs: prefs));
}

class OstpApp extends StatelessWidget {
  final SharedPreferences prefs;
  const OstpApp({super.key, required this.prefs});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'OSTP Client',
      debugShowCheckedModeBanner: false,
      theme: ThemeData(
        brightness: Brightness.dark,
        scaffoldBackgroundColor: const Color(0xFF030303),
        colorScheme: const ColorScheme.dark(
          primary: Color(0xFFF9FAFB),
          secondary: Color(0xFF10B981),
          surface: Color(0xFF09090B),
        ),
        fontFamily: 'Inter',
        useMaterial3: true,
      ),
      home: HomeScreen(prefs: prefs),
    );
  }
}
