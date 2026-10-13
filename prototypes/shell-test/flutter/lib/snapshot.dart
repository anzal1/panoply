import 'dart:async';
import 'dart:io';
import 'dart:ui' as ui;

import 'package:flutter/rendering.dart';
import 'package:flutter/widgets.dart';

import 'app.dart';
import 'bar.dart';
import 'model.dart';
import 'state.dart';
import 'controller.dart';

/// Offscreen verification renders: the real widget tree at pixelRatio 3 into
/// PNGs, light and dark. A flat color stands in for the native material.
Future<void> runSnapshots(List<Tool> tools, String dir) async {
  Directory(dir).createSync(recursive: true);
  final key = GlobalKey();
  final brightness = ValueNotifier<Brightness>(Brightness.dark);
  final state = BarState(tools);
  final motion = MotionPrefs();

  runApp(WidgetsApp(
    color: const Color(0xFF000000),
    debugShowCheckedModeBanner: false,
    builder: (context, _) => ValueListenableBuilder<Brightness>(
      valueListenable: brightness,
      builder: (context, b, _) {
        final dark = b == Brightness.dark;
        return RepaintBoundary(
          key: key,
          child: DecoratedBox(
            // wallpaper stand-in
            decoration: BoxDecoration(
              gradient: LinearGradient(
                begin: Alignment.topLeft,
                end: Alignment.bottomRight,
                colors: dark
                    ? const [Color(0xFF1D3A5C), Color(0xFF3B2A57)]
                    : const [Color(0xFFBFD4EA), Color(0xFFE6D3E8)],
              ),
            ),
            child: ClipRRect(
              borderRadius: BorderRadius.circular(20),
              child: DecoratedBox(
                // material stand-in
                decoration: BoxDecoration(color: dark ? const Color(0xE62A2A2E) : const Color(0xE6F4F4F6)),
                child: BarShell(
                  brightness: b,
                  child: BarView(state: state, motion: motion, onHide: () {}),
                ),
              ),
            ),
          ),
        );
      },
    ),
  ));

  Future<void> settle([int ms = 700]) async {
    for (var i = 0; i < 3; i++) {
      await WidgetsBinding.instance.endOfFrame;
    }
    await Future<void>.delayed(Duration(milliseconds: ms));
    await WidgetsBinding.instance.endOfFrame;
  }

  Future<void> shot(String name) async {
    await settle();
    final boundary = key.currentContext!.findRenderObject() as RenderRepaintBoundary;
    final image = await boundary.toImage(pixelRatio: 3);
    final bytes = (await image.toByteData(format: ui.ImageByteFormat.png))!;
    File('$dir/$name.png').writeAsBytesSync(bytes.buffer.asUint8List());
  }

  await settle(300);
  for (final b in [Brightness.dark, Brightness.light]) {
    brightness.value = b;
    final tag = b == Brightness.dark ? 'dark' : 'light';
    state.reset();
    await shot('$tag-1-empty');
    state.moveSelection(1);
    await shot('$tag-2-second-row');
    state.setQuery('pdf');
    state.moveSelection(2);
    await shot('$tag-3-query-pdf');
    state.setQuery('cmprs');
    await shot('$tag-4-fuzzy');
    state.setQuery('xyz');
    await shot('$tag-5-empty-state');
    state.setQuery('');
    state.setChip((name: 'Quarterly report, final draft (v2) - signed copy.pdf', ext: 'pdf'));
    state.dragExt.value = 'pdf';
    await shot('$tag-6-drop-ring');
    state.dragExt.value = null;
    state.runSelected();
    await Future<void>.delayed(const Duration(milliseconds: 100));
    await shot('$tag-7-chip-opened');
    state.reset();
    for (var i = 0; i < 24; i++) {
      state.moveSelection(1);
    }
    await shot('$tag-8-scrolled');
  }
  exit(0);
}
