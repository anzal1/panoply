import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import 'controller.dart';
import 'glide.dart';
import 'model.dart';
import 'state.dart';
import 'theme.dart';
import 'widgets.dart';

/// The whole bar: search row, results, footer, drop ring and hairline.
class BarView extends StatefulWidget {
  const BarView({
    super.key,
    required this.state,
    required this.motion,
    required this.onHide,
  });

  final BarState state;
  final MotionPrefs motion;
  final VoidCallback onHide;

  @override
  State<BarView> createState() => _BarViewState();
}

class _BarViewState extends State<BarView> {
  late final FocusNode _focus = FocusNode(debugLabel: 'query', onKeyEvent: _onKey);
  BarState get s => widget.state;

  @override
  void dispose() {
    _focus.dispose();
    super.dispose();
  }

  KeyEventResult _onKey(FocusNode node, KeyEvent e) {
    if (e is KeyUpEvent) return KeyEventResult.ignored;
    final k = e.logicalKey;
    final meta = HardwareKeyboard.instance.isMetaPressed;
    if (k == LogicalKeyboardKey.arrowDown) {
      s.moveSelection(1);
      return KeyEventResult.handled;
    }
    if (k == LogicalKeyboardKey.arrowUp) {
      s.moveSelection(-1);
      return KeyEventResult.handled;
    }
    if (k == LogicalKeyboardKey.enter || k == LogicalKeyboardKey.numpadEnter) {
      if (e is KeyDownEvent) s.runSelected();
      return KeyEventResult.handled;
    }
    if (k == LogicalKeyboardKey.escape) {
      if (e is KeyDownEvent) {
        if (s.text.text.isNotEmpty) {
          s.setQuery('');
        } else {
          widget.onHide();
        }
      }
      return KeyEventResult.handled;
    }
    if (meta && k == LogicalKeyboardKey.keyK) return KeyEventResult.handled; // Actions: not part of this test
    return KeyEventResult.ignored;
  }

  @override
  Widget build(BuildContext context) {
    final p = PaletteScope.of(context);
    return Stack(
      fit: StackFit.expand,
      children: [
        Column(
          children: [
            SearchRow(state: s, focus: _focus),
            _Hairline(color: p.separator),
            Expanded(child: ResultsArea(state: s, motion: widget.motion)),
            _Hairline(color: p.separator),
            Footer(state: s),
          ],
        ),
        // Drag-over ring: 2 pt accent at 40%.
        Positioned.fill(
          child: IgnorePointer(
            child: ValueListenableBuilder<String?>(
              valueListenable: s.dragExt,
              builder: (context, ext, _) => ValueListenableBuilder<Color>(
                valueListenable: s.accent,
                builder: (context, accent, _) => AnimatedOpacity(
                  opacity: ext != null ? 1 : 0,
                  duration: widget.motion.reduce ? const Duration(milliseconds: 90) : const Duration(milliseconds: 140),
                  curve: Curves.easeOut,
                  child: DecoratedBox(
                    decoration: BoxDecoration(
                      borderRadius: BorderRadius.circular(20),
                      border: Border.all(color: accent.withValues(alpha: 0.4), width: 2),
                    ),
                  ),
                ),
              ),
            ),
          ),
        ),
        // 0.5 pt inner hairline.
        Positioned.fill(
          child: IgnorePointer(
            child: DecoratedBox(
              decoration: BoxDecoration(
                borderRadius: BorderRadius.circular(20),
                border: Border.all(color: p.hairline, width: 0.5),
              ),
            ),
          ),
        ),
      ],
    );
  }
}

class _Hairline extends StatelessWidget {
  const _Hairline({required this.color});
  final Color color;
  @override
  Widget build(BuildContext context) => SizedBox(height: 0.5, width: double.infinity, child: ColoredBox(color: color));
}

// ---------------------------------------------------------------------------

class SearchRow extends StatelessWidget {
  const SearchRow({super.key, required this.state, required this.focus});
  final BarState state;
  final FocusNode focus;

  @override
  Widget build(BuildContext context) {
    final p = PaletteScope.of(context);
    final fieldStyle = sysStyle(20, FontWeight.w400, p.label);
    return SizedBox(
      height: 56,
      child: ValueListenableBuilder<({String name, String ext})?>(
        valueListenable: state.chip,
        builder: (context, chip, _) => Padding(
          padding: EdgeInsets.only(left: 20, right: chip == null ? 20 : 16),
          child: Row(
            children: [
              Icon(LucideIcons.search, size: 18, color: p.secondary),
              const SizedBox(width: 12), // text field starts at x 50
              Expanded(
                child: Stack(
                  alignment: Alignment.centerLeft,
                  children: [
                    ValueListenableBuilder<TextEditingValue>(
                      valueListenable: state.text,
                      builder: (context, v, _) => v.text.isEmpty
                          ? IgnorePointer(
                              child: Text(
                                'Search ${state.tools.length} tools or drop a file',
                                maxLines: 1,
                                softWrap: false,
                                style: sysStyle(20, FontWeight.w400, p.tertiary),
                              ),
                            )
                          : const SizedBox.shrink(),
                    ),
                    EditableText(
                      controller: state.text,
                      focusNode: focus,
                      style: fieldStyle,
                      cursorColor: p.label,
                      backgroundCursorColor: p.tertiary,
                      selectionColor: const Color(0xFF007AFF).withValues(alpha: 0.35),
                      cursorWidth: 1.5,
                      cursorRadius: const Radius.circular(1),
                      autofocus: true,
                      autocorrect: false,
                      enableSuggestions: false,
                      maxLines: 1,
                      showCursor: true,
                      scrollPadding: EdgeInsets.zero,
                      onChanged: (_) => state.queryChanged(),
                    ),
                  ],
                ),
              ),
              if (chip != null) ...[
                const SizedBox(width: 12),
                FileChip(name: chip.name, onClear: () => state.setChip(null)),
              ],
            ],
          ),
        ),
      ),
    );
  }
}

// ---------------------------------------------------------------------------

class ResultsArea extends StatefulWidget {
  const ResultsArea({super.key, required this.state, required this.motion});
  final BarState state;
  final MotionPrefs motion;

  @override
  State<ResultsArea> createState() => _ResultsAreaState();
}

class _ResultsAreaState extends State<ResultsArea> with TickerProviderStateMixin {
  final ScrollController _scroll = ScrollController();
  late final GlideSpring _spring = GlideSpring(this);
  late final AnimationController _fade = AnimationController(vsync: this, duration: const Duration(milliseconds: 90), value: 1);
  late final AnimationController _flash = AnimationController(vsync: this, duration: const Duration(milliseconds: 280));
  bool _hasPill = false;
  String _lastExt = '';

  BarState get s => widget.state;

  @override
  void initState() {
    super.initState();
    s.selected.addListener(_onSelect);
    s.entries.addListener(_onEntries);
    s.onRun = () => _flash.forward(from: 0);
    _retarget(initial: true);
  }

  @override
  void dispose() {
    s.selected.removeListener(_onSelect);
    s.entries.removeListener(_onEntries);
    _scroll.dispose();
    _spring.dispose();
    _fade.dispose();
    _flash.dispose();
    super.dispose();
  }

  void _onEntries() {
    // The list changed (typing, chip): back to the top. Keep the pill where it
    // is on screen, then let it glide to its new row.
    if (_scroll.hasClients && _scroll.offset != 0) {
      final off = _scroll.offset;
      _scroll.jumpTo(0);
      _spring.shift(-off);
    }
    _retarget();
  }

  void _onSelect() {
    if (s.inBatch) return; // handled by _onEntries
    _retarget();
    _scrollToSelection();
  }

  void _retarget({bool initial = false}) {
    final i = s.selected.value;
    final list = s.entries.value;
    if (i < 0 || i >= list.length) {
      _hasPill = false;
      _spring.repaint();
      return;
    }
    final target = list[i].top;
    if (initial || !_hasPill || s.instant) {
      _spring.jumpTo(target);
      if (!initial && _fade.status != AnimationStatus.forward) {
        if (widget.motion.reduce) {
          _fade.forward(from: 0);
        } else {
          _fade.value = 1;
        }
      }
      _hasPill = true;
      return;
    }
    if (widget.motion.reduce) {
      // Reduce Motion: no travel, a short fade at the new position instead.
      _spring.jumpTo(target);
      _fade.forward(from: 0);
    } else {
      _fade.value = 1;
      _spring.glideTo(target);
    }
  }

  void _scrollToSelection() {
    if (!_scroll.hasClients) return;
    final list = s.entries.value;
    final i = s.selected.value;
    if (i < 0 || i >= list.length) return;
    final pos = _scroll.position;
    var top = list[i].top;
    final bottom = list[i].bottom;
    // Keep a little margin; reveal the section header when moving into a section.
    if (i > 0 && list[i - 1].isHeader) top = list[i - 1].top - 4;
    double? to;
    if (top - 12 < pos.pixels) {
      to = top - 12;
    } else if (bottom + 12 > pos.pixels + pos.viewportDimension) {
      to = bottom + 12 - pos.viewportDimension;
    }
    if (i == 1 && list[0].isHeader) to = to == null ? null : 0;
    if (to == null) return;
    to = to.clamp(0.0, pos.maxScrollExtent);
    if (widget.motion.reduce) {
      _scroll.jumpTo(to);
    } else {
      _scroll.animateTo(to, duration: const Duration(milliseconds: 160), curve: Curves.easeOutCubic);
    }
  }

  @override
  Widget build(BuildContext context) {
    final p = PaletteScope.of(context);
    return ValueListenableBuilder<List<Entry>>(
      valueListenable: s.entries,
      builder: (context, entries, _) {
        return ValueListenableBuilder<String?>(
          valueListenable: s.dragExt,
          builder: (context, ext, _) {
            if (ext != null) _lastExt = ext;
            final dragging = ext != null;
            return Stack(
              fit: StackFit.expand,
              children: [
                AnimatedOpacity(
                  opacity: dragging ? 0.12 : 1,
                  duration: const Duration(milliseconds: 140),
                  curve: Curves.easeOut,
                  child: entries.isEmpty ? _empty(p) : _list(p, entries),
                ),
                IgnorePointer(
                  child: AnimatedOpacity(
                    opacity: dragging ? 1 : 0,
                    duration: const Duration(milliseconds: 140),
                    curve: Curves.easeOut,
                    child: Center(
                      child: Text(
                        'Drop to see tools for .$_lastExt',
                        style: sysStyle(13, FontWeight.w500, p.secondary),
                      ),
                    ),
                  ),
                ),
              ],
            );
          },
        );
      },
    );
  }

  Widget _empty(Palette p) {
    final q = s.text.text.trim();
    return Center(
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          Text('No tools match “$q”', maxLines: 1, style: sysStyle(13, FontWeight.w400, p.secondary)),
          const SizedBox(height: 4),
          Text('Try “compress” or drop a file', maxLines: 1, style: sysStyle(12, FontWeight.w400, p.tertiary)),
        ],
      ),
    );
  }

  Widget _list(Palette p, List<Entry> entries) {
    return ClipRect(
      child: Stack(
        fit: StackFit.expand,
        children: [
          // Selection pill: lives below the rows, paints from the spring and the scroll offset only.
          RepaintBoundary(
            child: CustomPaint(
              painter: _PillPainter(
                spring: _spring,
                scroll: _scroll,
                fade: _fade,
                flash: _flash,
                fill: p.pillFill,
                flashColor: p.label,
                hasPill: () => _hasPill,
              ),
            ),
          ),
          ScrollConfiguration(
            behavior: const _QuietScroll(),
            child: ListView.builder(
              controller: _scroll,
              padding: const EdgeInsets.only(top: kListTopPad, bottom: kListBottomPad),
              physics: const ClampingScrollPhysics(),
              itemCount: entries.length,
              itemExtentBuilder: (i, _) => entries[i].height,
              itemBuilder: (context, i) {
                final e = entries[i];
                if (e.isHeader) return HeaderTile(e.title!);
                return RowTile(
                  key: ValueKey(e.tool!.id),
                  index: i,
                  entry: e,
                  selected: s.selected,
                  onTap: () {
                    s.select(i);
                    s.runSelected();
                  },
                );
              },
            ),
          ),
        ],
      ),
    );
  }
}

class _QuietScroll extends ScrollBehavior {
  const _QuietScroll();
  @override
  Widget buildScrollbar(BuildContext context, Widget child, ScrollableDetails details) => child;
  @override
  Widget buildOverscrollIndicator(BuildContext context, Widget child, ScrollableDetails details) => child;
}

class _PillPainter extends CustomPainter {
  _PillPainter({
    required this.spring,
    required this.scroll,
    required this.fade,
    required this.flash,
    required this.fill,
    required this.flashColor,
    required this.hasPill,
  }) : super(repaint: Listenable.merge([spring, scroll, fade, flash]));

  final GlideSpring spring;
  final ScrollController scroll;
  final AnimationController fade;
  final AnimationController flash;
  final Color fill;
  final Color flashColor;
  final bool Function() hasPill;

  @override
  void paint(Canvas canvas, Size size) {
    if (!hasPill()) return;
    final offset = scroll.hasClients ? scroll.offset : 0.0;
    final y = spring.x - offset;
    final rect = RRect.fromRectAndRadius(
      Rect.fromLTWH(8, y, size.width - 16, kRowHeight),
      const Radius.circular(10),
    );
    final f = fade.value;
    canvas.drawRRect(rect, Paint()..color = fill.withValues(alpha: fill.a * f));
    final fl = flash.isAnimating ? (1 - Curves.easeOut.transform(flash.value)) : 0.0;
    if (fl > 0) {
      canvas.drawRRect(rect, Paint()..color = flashColor.withValues(alpha: flashColor.a * 0.16 * fl));
    }
  }

  @override
  bool shouldRepaint(_PillPainter old) => old.fill != fill || old.flashColor != flashColor;
}

// ---------------------------------------------------------------------------

class Footer extends StatelessWidget {
  const Footer({super.key, required this.state});
  final BarState state;

  @override
  Widget build(BuildContext context) {
    final p = PaletteScope.of(context);
    final label = sysStyle(12, FontWeight.w400, p.secondary);
    return SizedBox(
      height: 40,
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 20),
        child: Row(
          children: [
            // App glyph: a simple rounded square.
            DecoratedBox(
              decoration: BoxDecoration(
                borderRadius: BorderRadius.circular(4.5),
                color: p.secondary,
              ),
              child: const SizedBox(width: 16, height: 16),
            ),
            const SizedBox(width: 8),
            Expanded(
              child: ValueListenableBuilder<String?>(
                valueListenable: state.opened,
                builder: (context, opened, _) =>
                    Text(opened ?? 'Arsenal', maxLines: 1, softWrap: false, overflow: TextOverflow.ellipsis, style: label),
              ),
            ),
            Text('Open', style: label),
            const SizedBox(width: 6),
            const KeyCap('↵'),
            const SizedBox(width: 16),
            Text('Actions', style: label),
            const SizedBox(width: 6),
            const KeyCap('⌘'),
            const SizedBox(width: 3),
            const KeyCap('K'),
          ],
        ),
      ),
    );
  }
}
