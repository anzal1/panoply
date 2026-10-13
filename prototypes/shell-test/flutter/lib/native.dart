import 'package:flutter/services.dart';

/// Bridge to the Swift side (window, material, hotkey, file drops).
class Native {
  Native._() {
    _ch.setMethodCallHandler(_onCall);
  }
  static final Native instance = Native._();

  static const _ch = MethodChannel('arsenal/native');

  /// Native to Dart events.
  VoidCallback? onHotkey;
  VoidCallback? onResignKey;
  void Function(String ext, Color accent)? onDragEnter;
  VoidCallback? onDragExit;
  void Function(String name, String ext)? onDrop;
  void Function(bool reduce)? onReduceMotion;

  Future<dynamic> _onCall(MethodCall call) async {
    switch (call.method) {
      case 'hotkey':
        onHotkey?.call();
      case 'resignKey':
        onResignKey?.call();
      case 'dragEnter':
        final a = call.arguments as Map;
        onDragEnter?.call(a['ext'] as String, Color(a['accent'] as int));
      case 'dragExit':
        onDragExit?.call();
      case 'drop':
        final a = call.arguments as Map;
        onDrop?.call(a['name'] as String, a['ext'] as String);
      case 'reduceMotion':
        onReduceMotion?.call(call.arguments as bool);
    }
    return null;
  }

  /// Fire and forget: the window is ordered front on the platform thread.
  void show() => _ch.invokeMethod<void>('show');
  void hide() => _ch.invokeMethod<void>('hide');

  /// The first frame is up: the native side orders the pre-created window out.
  Future<Map?> ready() => _ch.invokeMethod<Map>('ready');
}
