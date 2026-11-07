import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/scheduler.dart';

/// Profiling aid, off unless the app is built with `--dart-define=FRAME_STATS=true`: logs build and raster
/// times of the frames drawn in each 2 s window (tag FRAMESTATS), so scenarios can be compared before and after.
void installFrameStats() {
  if (!const bool.fromEnvironment('FRAME_STATS')) return;
  final List<FrameTiming> window = <FrameTiming>[];
  SchedulerBinding.instance.addTimingsCallback(window.addAll);
  Timer.periodic(const Duration(seconds: 2), (Timer _) {
    if (window.isEmpty) return;
    final List<double> build = window.map((FrameTiming t) => t.buildDuration.inMicroseconds / 1000).toList()..sort();
    final List<double> raster = window.map((FrameTiming t) => t.rasterDuration.inMicroseconds / 1000).toList()..sort();
    final List<double> total = window.map((FrameTiming t) => t.totalSpan.inMicroseconds / 1000).toList()..sort();
    double avg(List<double> v) => v.reduce((double a, double b) => a + b) / v.length;
    double p90(List<double> v) => v[((v.length - 1) * 0.9).floor()];
    final int jank = window.where((FrameTiming t) => t.totalSpan.inMicroseconds > 8333).length;
    debugPrint('FRAMESTATS frames=${window.length} build_avg=${avg(build).toStringAsFixed(1)} build_p90=${p90(build).toStringAsFixed(1)} '
        'raster_avg=${avg(raster).toStringAsFixed(1)} raster_p90=${p90(raster).toStringAsFixed(1)} total_p90=${p90(total).toStringAsFixed(1)} over8ms=$jank');
    window.clear();
  });
}
