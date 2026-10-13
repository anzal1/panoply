import 'dart:async';

import 'package:flutter/widgets.dart';

import 'fuzzy.dart';
import 'model.dart';

const _categoryOrder = ['PDF', 'Image', 'Video', 'Audio', 'Files', 'Text', 'Capture'];

/// The searchable content of the bar. UI pieces listen to the narrow notifier
/// they care about, so typing only rebuilds the list and the query field.
class BarState {
  BarState(this.tools) {
    _recompute();
  }

  final List<Tool> tools;

  final TextEditingController text = TextEditingController();

  /// Current list lines (headers and rows).
  final ValueNotifier<List<Entry>> entries = ValueNotifier(const []);

  /// Index into [entries] of the selected row, or -1.
  final ValueNotifier<int> selected = ValueNotifier(-1);

  /// Drives the footer's leading text ("Opened Compress PDF").
  final ValueNotifier<String?> opened = ValueNotifier(null);

  /// Chip for a dropped file.
  final ValueNotifier<({String name, String ext})?> chip = ValueNotifier(null);

  /// Drag-over state: null when idle, else the extension being dragged.
  final ValueNotifier<String?> dragExt = ValueNotifier(null);
  final ValueNotifier<Color> accent = ValueNotifier(const Color(0xFF007AFF));

  /// True while a list swap is assigning selection and entries together.
  bool inBatch = false;

  /// True while the bar is being reset for a fresh show: the pill must not travel.
  bool instant = false;

  Timer? _openedTimer;
  VoidCallback? onRun;

  String get query => text.text;
  bool get isFiltering => text.text.trim().isNotEmpty;

  Tool? get selectedTool {
    final i = selected.value;
    final list = entries.value;
    return i >= 0 && i < list.length ? list[i].tool : null;
  }

  void setQuery(String q) {
    if (text.text == q) return;
    text.value = TextEditingValue(text: q, selection: TextSelection.collapsed(offset: q.length));
    queryChanged();
  }

  /// Call after the text controller changed.
  void queryChanged() {
    _recompute();
  }

  void setChip(({String name, String ext})? c) {
    chip.value = c;
    _recompute();
  }

  void reset() {
    text.value = TextEditingValue.empty;
    chip.value = null;
    dragExt.value = null;
    instant = true;
    _recompute();
    instant = false;
  }

  void moveSelection(int delta) {
    final list = entries.value;
    var i = selected.value;
    final step = delta.sign;
    for (var n = delta.abs(); n > 0; n--) {
      var j = i + step;
      while (j >= 0 && j < list.length && list[j].isHeader) {
        j += step;
      }
      if (j < 0 || j >= list.length) break; // clamped
      i = j;
    }
    if (i != selected.value) selected.value = i;
  }

  void select(int index) {
    if (index != selected.value) selected.value = index;
  }

  /// "Runs" the selected tool.
  void runSelected() {
    final t = selectedTool;
    if (t == null) return;
    opened.value = 'Opened ${t.name}';
    _openedTimer?.cancel();
    _openedTimer = Timer(const Duration(milliseconds: 1200), () => opened.value = null);
    onRun?.call();
  }

  void _recompute() {
    final q = text.text.toLowerCase().replaceAll(RegExp(r'\s+'), '');
    final c = chip.value;
    final pool = c == null ? tools : [for (final t in tools) if (t.acceptsExt(c.ext)) t];
    final out = <Entry>[];
    var y = kListTopPad;

    if (q.isEmpty) {
      for (final cat in _categoryOrder) {
        final inCat = [for (final t in pool) if (t.category == cat) t];
        if (inCat.isEmpty) continue;
        out.add(Entry.header(cat, y));
        y += kHeaderHeight;
        for (final t in inCat) {
          out.add(Entry.row(t, const [], y));
          y += kRowHeight;
        }
      }
    } else {
      final scored = <(double, Tool, List<int>)>[];
      for (final t in pool) {
        final r = fuzzyMatch(q, t.name);
        var best = r?.score ?? -1e9;
        final pos = r?.positions ?? const <int>[];
        // Secondary fields count less and never highlight.
        for (final s in [t.subtitle, t.category, t.id.replaceAll('-', ' ')]) {
          final sc = substringScore(q, s);
          if (sc > 0 && sc * 0.5 > best) best = sc * 0.5;
        }
        if (best > -1e8) scored.add((best, t, pos));
      }
      scored.sort((a, b) {
        final c = b.$1.compareTo(a.$1);
        return c != 0 ? c : tools.indexOf(a.$2).compareTo(tools.indexOf(b.$2));
      });
      if (scored.isNotEmpty) {
        out.add(Entry.header('Best matches', y));
        y += kHeaderHeight;
        for (final s in scored) {
          out.add(Entry.row(s.$2, s.$3, y));
          y += kRowHeight;
        }
      }
    }
    inBatch = true;
    selected.value = out.indexWhere((e) => !e.isHeader);
    entries.value = out;
    inBatch = false;
  }

  double get contentHeight {
    final l = entries.value;
    return l.isEmpty ? 0 : l.last.bottom + kListBottomPad;
  }
}
