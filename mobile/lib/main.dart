import 'dart:async';
import 'dart:ui' show FlutterView;

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'app/app.dart';
import 'debug/frame_stats.dart';
import 'debug/kb_trace.dart';
import 'debug/voice_bench.dart';

void main() {
  WidgetsFlutterBinding.ensureInitialized();
  unawaited(_lockPhonesToPortrait());
  installFrameStats();
  installKbTrace();
  runApp(const ProviderScope(child: AgentConsoleApp()));
  unawaited(runVoiceBenchIfRequested());
}

/// Phones stay upright; tablets (iPad) turn, since they have a two-column landscape layout. On iOS Info.plist already
/// limits the iPhone, but Android has no per-size setting, so the phone is told here. 600 dp is Android's tablet line.
Future<void> _lockPhonesToPortrait() async {
  final Iterable<FlutterView> views = WidgetsBinding.instance.platformDispatcher.views;
  if (views.isEmpty) return;
  final FlutterView view = views.first;
  final double shortest = (view.physicalSize / view.devicePixelRatio).shortestSide;
  if (shortest >= 600) return;
  await SystemChrome.setPreferredOrientations(<DeviceOrientation>[DeviceOrientation.portraitUp]);
}
