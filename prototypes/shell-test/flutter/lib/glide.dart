import 'dart:math' as math;

import 'package:flutter/foundation.dart';
import 'package:flutter/scheduler.dart';

/// A critically damped spring (damping ratio exactly 1): reaches its target
/// with no overshoot. Closed-form step, so it is exact at any frame time.
///
/// omega = 34 rad/s settles within a quarter point of the target in about
/// 220 ms (95% of the distance is covered in roughly 140 ms).
class GlideSpring extends ChangeNotifier {
  GlideSpring(TickerProvider vsync, {this.omega = 34.0}) {
    _ticker = vsync.createTicker(_tick);
  }

  final double omega;
  late final Ticker _ticker;

  double x = 0;
  double v = 0;
  double _target = 0;
  Duration _last = Duration.zero;

  double get target => _target;
  bool get isMoving => _ticker.isActive;

  /// Jump with no animation.
  void jumpTo(double value) {
    _ticker.stop();
    x = _target = value;
    v = 0;
    notifyListeners();
  }

  /// Re-express the current position after the scroll origin moved.
  void shift(double d) => x += d;

  void repaint() => notifyListeners();

  void glideTo(double value) {
    _target = value;
    if (!_ticker.isActive) {
      _last = Duration.zero;
      _ticker.start();
    }
  }

  void _tick(Duration elapsed) {
    var dt = (elapsed - _last).inMicroseconds / 1e6;
    if (_last == Duration.zero) dt = 1 / 120;
    _last = elapsed;
    if (dt > 0.05) dt = 0.05;
    final e = x - _target;
    final b = v + omega * e;
    final decay = math.exp(-omega * dt);
    x = _target + (e + b * dt) * decay;
    v = (v - b * omega * dt) * decay;
    if ((x - _target).abs() < 0.02 && v.abs() < 0.5) {
      x = _target;
      v = 0;
      _ticker.stop();
    }
    notifyListeners();
  }

  @override
  void dispose() {
    _ticker.dispose();
    super.dispose();
  }
}
