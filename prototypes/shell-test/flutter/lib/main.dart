import 'dart:io';

import 'package:flutter/widgets.dart';

import 'app.dart';
import 'model.dart';
import 'snapshot.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  final tools = await loadTools();
  final snapDir = Platform.environment['ARSENAL_SNAPSHOT_DIR'];
  if (snapDir != null && snapDir.isNotEmpty) {
    // Verification mode: render the bar offscreen to PNGs (3x) and exit.
    await runSnapshots(tools, snapDir);
    return;
  }
  runApp(ArsenalRoot(tools: tools));
}
