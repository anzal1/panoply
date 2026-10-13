import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/scheduler.dart';
import 'package:flutter/widgets.dart';

import 'native.dart';
import 'probe.dart';
import 'state.dart';

const int kControlPort = 47813;

/// Show/hide, the hotkey and control-port plumbing. [toggle] is the single
/// entry point shared by the hotkey and `GET /toggle`.
class BarController {
  BarController(this.state, this.motion, this.showCtl);

  final BarState state;
  final MotionPrefs motion;
  final AnimationController showCtl;

  bool visible = false;
  final List<Probe> showProbes = [];
  Timer? _hideGuard;

  void start() {
    final n = Native.instance;
    n.onHotkey = () => toggle(t0: nowMs());
    n.onResignKey = () {
      if (visible) hide();
    };
    n.onReduceMotion = (r) => motion.nativeReduce = r;
    n.onDragEnter = (ext, accent) {
      state.accent.value = accent;
      state.dragExt.value = ext;
    };
    n.onDragExit = () => state.dragExt.value = null;
    n.onDrop = (name, ext) {
      state.dragExt.value = null;
      state.setChip((name: name, ext: ext));
    };
  }

  /// Hotkey and `/toggle` both land here. Returns the probe when showing.
  Probe? toggle({double? t0}) {
    if (visible) {
      hide();
      return null;
    }
    return show(t0: t0);
  }

  Probe? show({double? t0, bool measure = true}) {
    final probe = measure ? FrameProbes.instance.start(t0: t0) : null;
    if (probe != null) showProbes.add(probe);
    _hideGuard?.cancel();
    visible = true;
    state.reset();
    showCtl.value = 0;
    Native.instance.show();
    showCtl.forward();
    return probe;
  }

  void hide() {
    if (!visible) return;
    visible = false;
    Native.instance.hide();
    showCtl.reverse();
  }
}

/// Reduce Motion, from either the engine's accessibility features or AppKit.
class MotionPrefs {
  bool nativeReduce = false;
  bool get reduce =>
      nativeReduce || SchedulerBinding.instance.platformDispatcher.accessibilityFeatures.disableAnimations;
}

String _json(Object o) => jsonEncode(o);

double _pct(List<double> sorted, double p) {
  if (sorted.isEmpty) return 0;
  final rank = (p * sorted.length).ceil().clamp(1, sorted.length);
  return sorted[rank - 1];
}

double _r2(double v) => (v * 100).roundToDouble() / 100;

/// Tiny HTTP control server for measurement: /toggle, /metrics, /selftest.
class ControlServer {
  ControlServer(this.bar);
  final BarController bar;
  HttpServer? _server;
  var _selftestRunning = false;

  Future<void> start() async {
    _server = await HttpServer.bind(InternetAddress.loopbackIPv4, kControlPort, shared: false);
    _server!.listen(_handle);
  }

  Future<void> _handle(HttpRequest req) async {
    final t0 = nowMs(); // first thing: before any work
    final res = req.response;
    try {
      switch (req.uri.path) {
        case '/toggle':
          final probe = bar.toggle(t0: t0);
          final visible = bar.visible;
          _send(res, _json({'visible': visible, 't0': t0, 'recorded': probe != null}));
        case '/metrics':
          await FrameProbes.instance.settle(bar.showProbes);
          _send(res, _json({
            'show_ms': [for (final p in bar.showProbes) if (p.ms != null) _r2(p.ms!)],
          }));
        case '/selftest':
          if (_selftestRunning) {
            res.statusCode = 409;
            _send(res, _json({'error': 'selftest already running'}));
          } else {
            _selftestRunning = true;
            try {
              _send(res, _json(await _selftest()));
            } finally {
              _selftestRunning = false;
            }
          }
        default:
          res.statusCode = 404;
          _send(res, _json({'error': 'not found'}));
      }
    } catch (e) {
      res.statusCode = 500;
      _send(res, _json({'error': '$e'}));
    }
  }

  void _send(HttpResponse res, String body) {
    res.headers.contentType = ContentType.json;
    res.headers.set('Cache-Control', 'no-store');
    res.write(body);
    res.close();
  }

  Future<Map<String, Object>> _selftest() async {
    if (!bar.visible) {
      bar.show(measure: false);
    }
    // Let the show animation finish and the first frames settle.
    await Future<void>.delayed(const Duration(milliseconds: 320));
    bar.state.reset();
    await WidgetsBinding.instance.endOfFrame;

    final probes = <Probe>[];
    Future<void> step(String q) async {
      final p = FrameProbes.instance.start();
      probes.add(p);
      bar.state.setQuery(q); // the state change
      await WidgetsBinding.instance.endOfFrame;
      // Gap so consecutive updates land in distinct frames ("one at a time").
      await Future<void>.delayed(const Duration(milliseconds: 24));
    }

    for (final word in ['compress pdf', 'heic to jpg', 'trim video']) {
      for (var i = 1; i <= word.length; i++) {
        await step(word.substring(0, i));
      }
      for (var i = word.length - 1; i >= 0; i--) {
        await step(word.substring(0, i));
      }
    }
    await FrameProbes.instance.settle(probes);
    if (Platform.environment['ARSENAL_DEBUG'] != null) {
      final raster = probes.where((p) => p.resolved).length;
      stderr.writeln('selftest: ${probes.length} probes, $raster via raster timings, ${probes.length - raster} via endOfFrame fallback');
    }
    final ms = [for (final p in probes) if (p.ms != null) p.ms!]..sort();
    return {
      'n': ms.length,
      'p50': _r2(_pct(ms, 0.50)),
      'p95': _r2(_pct(ms, 0.95)),
      'max': _r2(ms.isEmpty ? 0 : ms.last),
    };
  }
}
