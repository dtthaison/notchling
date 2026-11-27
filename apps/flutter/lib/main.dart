import 'dart:async';
import 'dart:ui' show FlutterView;

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'app/app.dart';
import 'app/l10n_holder.dart';
import 'debug/frame_stats.dart';
import 'debug/kb_trace.dart';
import 'debug/voice_bench.dart';

void main() {
  WidgetsFlutterBinding.ensureInitialized();
  L10nHolder.watch();
  unawaited(_lockPhonesToPortrait());
  installFrameStats();
  installKbTrace();
  runApp(const ProviderScope(child: AgentConsoleApp()));
  unawaited(runVoiceBenchIfRequested());
}

Future<void> _lockPhonesToPortrait() async {
  final Iterable<FlutterView> views = WidgetsBinding.instance.platformDispatcher.views;
  if (views.isEmpty) return;
  final FlutterView view = views.first;
  final double shortest = (view.physicalSize / view.devicePixelRatio).shortestSide;
  if (shortest >= 600) return;
  await SystemChrome.setPreferredOrientations(<DeviceOrientation>[DeviceOrientation.portraitUp]);
}
