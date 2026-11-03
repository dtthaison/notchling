import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'app/app.dart';
import 'debug/frame_stats.dart';
import 'debug/kb_trace.dart';
import 'debug/voice_bench.dart';

void main() {
  WidgetsFlutterBinding.ensureInitialized();
  installFrameStats();
  installKbTrace();
  runApp(const ProviderScope(child: AgentConsoleApp()));
  unawaited(runVoiceBenchIfRequested());
}
