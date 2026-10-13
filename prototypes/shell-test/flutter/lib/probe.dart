import 'dart:async';
import 'dart:ui';

import 'package:flutter/scheduler.dart';
import 'package:flutter/widgets.dart';

double nowMs() => DateTime.now().microsecondsSinceEpoch / 1000;

/// One "state change to presented frame" measurement.
///
/// Clock choice, in order of preference:
///  1. `FrameTiming` raster finish for the first frame whose build started after
///     the state change. `FramePhase.rasterFinishWallTime` is a wall clock
///     (epoch microseconds), so t1 is on the same clock as t0. The frame's
///     build start is mapped to wall time with the same monotonic to wall offset.
///  2. If the engine's batched timing report has not arrived (it flushes at
///     most once a second in release builds), the time after
///     `addPostFrameCallback` plus `await endOfFrame` is used instead.
class Probe {
  Probe(this.t0);
  final double t0;
  double? fallbackT1;
  double? rasterT1;
  Completer<void>? _done;

  double? get ms {
    final t1 = rasterT1 ?? fallbackT1;
    return t1 == null ? null : t1 - t0;
  }

  bool get resolved => rasterT1 != null;
  Future<void> get whenResolved => (_done ??= Completer<void>()).future;
  void _resolve() => _done?.complete();
}

class FrameProbes {
  FrameProbes._() {
    SchedulerBinding.instance.addTimingsCallback(_onTimings);
  }
  static final FrameProbes instance = FrameProbes._();

  final List<({double buildStart, double rasterFinish})> _timings = [];
  final List<Probe> _pending = [];

  void _onTimings(List<FrameTiming> ts) {
    for (final t in ts) {
      final rasterWallMs = t.timestampInMicroseconds(FramePhase.rasterFinishWallTime) / 1000;
      final rasterMono = t.timestampInMicroseconds(FramePhase.rasterFinish);
      final offsetMs = rasterWallMs - rasterMono / 1000;
      final buildWallMs = t.timestampInMicroseconds(FramePhase.buildStart) / 1000 + offsetMs;
      _timings.add((buildStart: buildWallMs, rasterFinish: rasterWallMs));
    }
    if (_timings.length > 2048) _timings.removeRange(0, _timings.length - 1024);
    for (final p in List.of(_pending)) {
      for (final t in _timings) {
        // 0.25 ms of slack for clock conversion rounding.
        if (t.buildStart >= p.t0 - 0.25) {
          p.rasterT1 = t.rasterFinish;
          _pending.remove(p);
          p._resolve();
          break;
        }
      }
    }
  }

  /// Starts a probe at the moment of the state change (call right before
  /// mutating state). Completes its fallback once the frame has been presented.
  Probe start({double? t0}) {
    final p = Probe(t0 ?? nowMs());
    _pending.add(p);
    SchedulerBinding.instance.addPostFrameCallback((_) async {
      await WidgetsBinding.instance.endOfFrame;
      p.fallbackT1 ??= nowMs();
    });
    return p;
  }

  /// Wait for the engine's timing report for [probes], up to [timeout].
  Future<void> settle(Iterable<Probe> probes, {Duration timeout = const Duration(milliseconds: 1600)}) async {
    final open = [for (final p in probes) if (!p.resolved) p];
    if (open.isEmpty) return;
    // Make sure a frame goes out so a first-of-batch report is not delayed.
    await Future.any([
      Future.wait([for (final p in open) p.whenResolved]),
      Future<void>.delayed(timeout),
    ]);
    for (final p in open) {
      if (!p.resolved) _pending.remove(p);
    }
  }
}
