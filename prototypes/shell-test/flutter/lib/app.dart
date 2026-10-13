import 'dart:async';
import 'dart:io';

import 'package:flutter/widgets.dart';

import 'bar.dart';
import 'controller.dart';
import 'model.dart';
import 'native.dart';
import 'probe.dart';
import 'state.dart';
import 'theme.dart';

/// Show curve from the spec: cubic-bezier(0.2, 0.9, 0.25, 1).
const Curve kShowCurve = Cubic(0.2, 0.9, 0.25, 1.0);

/// Palette, default text style and the show/hide transform around [BarView].
class BarShell extends StatelessWidget {
  const BarShell({super.key, required this.brightness, required this.child, this.showCtl, this.reduceMotion = false});
  final Brightness brightness;
  final Widget child;
  final AnimationController? showCtl;
  final bool reduceMotion;

  @override
  Widget build(BuildContext context) {
    final p = Palette.of(brightness);
    Widget body = RepaintBoundary(child: child);
    final ctl = showCtl;
    if (ctl != null) {
      final inner = body;
      body = AnimatedBuilder(
        animation: ctl,
        child: inner,
        builder: (context, child) {
          if (reduceMotion) return child!;
          final v = ctl.value;
          final double scale;
          final double dy;
          if (ctl.status == AnimationStatus.reverse) {
            // Hide: 120 ms ease-in, scale to 0.99 (opacity is the native window alpha).
            scale = 1 - 0.01 * Curves.easeIn.transform(1 - v);
            dy = 0;
          } else {
            final c = kShowCurve.transform(v);
            scale = 0.985 + 0.015 * c;
            dy = -6 * (1 - c);
          }
          return Transform(
            alignment: Alignment.center,
            transform: Matrix4.translationValues(0, dy, 0)..multiply(Matrix4.diagonal3Values(scale, scale, 1)),
            child: child,
          );
        },
      );
    }
    return PaletteScope(
      palette: p,
      child: DefaultTextStyle(style: sysStyle(13, FontWeight.w400, p.label), child: body),
    );
  }
}

class ArsenalRoot extends StatefulWidget {
  const ArsenalRoot({super.key, required this.tools});
  final List<Tool> tools;

  @override
  State<ArsenalRoot> createState() => _ArsenalRootState();
}

class _ArsenalRootState extends State<ArsenalRoot> with SingleTickerProviderStateMixin {
  late final AnimationController _show = AnimationController(
    vsync: this,
    duration: const Duration(milliseconds: 180),
    reverseDuration: const Duration(milliseconds: 120),
  );
  late final BarState _state = BarState(widget.tools);
  final MotionPrefs _motion = MotionPrefs();
  late final BarController _bar = BarController(_state, _motion, _show);
  late final ControlServer _server = ControlServer(_bar);

  @override
  void initState() {
    super.initState();
    _bar.start();
    unawaited(_server.start());
    WidgetsBinding.instance.addObserver(_Observer(() => setState(() {})));
    unawaited(_firstFrame());
  }

  /// First presented frame: stamp the launch file, then tell the native side to
  /// order the pre-created window out (it was ordered in at alpha 0 so the
  /// engine could render).
  Future<void> _firstFrame() async {
    await WidgetsBinding.instance.waitUntilFirstFrameRasterized;
    final stamp = nowMs();
    try {
      final dir = _resultsDir();
      Directory(dir).createSync(recursive: true);
      File('$dir/flutter-launch.txt').writeAsStringSync('${stamp.toStringAsFixed(3)}\n', mode: FileMode.append);
    } catch (_) {}
    final info = await Native.instance.ready();
    if (info != null) _motion.nativeReduce = info['reduceMotion'] == true;
  }

  @override
  void dispose() {
    _show.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return WidgetsApp(
      color: const Color(0x00000000),
      debugShowCheckedModeBanner: false,
      builder: (context, _) {
        final brightness = MediaQuery.platformBrightnessOf(context);
        return BarShell(
          brightness: brightness,
          showCtl: _show,
          reduceMotion: _motion.reduce,
          child: BarView(state: _state, motion: _motion, onHide: _bar.hide),
        );
      },
    );
  }
}

class _Observer with WidgetsBindingObserver {
  _Observer(this.onChange);
  final VoidCallback onChange;
  @override
  void didChangeAccessibilityFeatures() => onChange();
}

String _resultsDir() {
  final env = Platform.environment['ARSENAL_RESULTS_DIR'];
  if (env != null && env.isNotEmpty) return env;
  // Walk up from the executable until the folder that holds SPEC.md is found.
  var d = File(Platform.resolvedExecutable).parent;
  for (var i = 0; i < 10; i++) {
    if (File('${d.path}/SPEC.md').existsSync()) return '${d.path}/results';
    final up = d.parent;
    if (up.path == d.path) break;
    d = up;
  }
  return '/Users/anzalhussainabidi/personal/arsenal/shell-test/results';
}
