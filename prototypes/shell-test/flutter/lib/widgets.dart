import 'package:flutter/widgets.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import 'lucide_map.dart';
import 'model.dart';
import 'theme.dart';

/// Small key cap: 11 pt, radius 5, fill primary 8%, at least 18 x 18.
class KeyCap extends StatelessWidget {
  const KeyCap(this.label, {super.key, this.fill});
  final String label;
  final Color? fill;

  @override
  Widget build(BuildContext context) {
    final p = PaletteScope.of(context);
    return ConstrainedBox(
      constraints: const BoxConstraints(minWidth: 18),
      child: SizedBox(
        height: 18,
        child: DecoratedBox(
          decoration: BoxDecoration(color: fill ?? p.keyCapFill, borderRadius: BorderRadius.circular(5)),
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 5),
            child: Center(
              widthFactor: 1,
              child: Text(label, maxLines: 1, style: sysStyle(11, FontWeight.w500, p.secondary)),
            ),
          ),
        ),
      ),
    );
  }
}

class HeaderTile extends StatelessWidget {
  const HeaderTile(this.title, {super.key});
  final String title;

  @override
  Widget build(BuildContext context) {
    final p = PaletteScope.of(context);
    return Padding(
      padding: const EdgeInsets.only(left: 20, right: 20, top: 4),
      child: Align(
        alignment: Alignment.centerLeft,
        child: Text(title, maxLines: 1, style: sysStyle(11, FontWeight.w500, p.tertiary)),
      ),
    );
  }
}

class IconTile extends StatelessWidget {
  const IconTile({super.key, required this.tool});
  final Tool tool;

  @override
  Widget build(BuildContext context) {
    final p = PaletteScope.of(context);
    final tint = p.tint(tool.category);
    return Container(
      width: 28,
      height: 28,
      alignment: Alignment.center,
      decoration: BoxDecoration(
        color: tint.withValues(alpha: 0.16),
        borderRadius: BorderRadius.circular(7),
      ),
      child: Icon(lucideByName[tool.lucide] ?? LucideIcons.shapes, size: 15, color: tint),
    );
  }
}

/// A result row. Rebuilds itself only when its own selected state flips.
class RowTile extends StatefulWidget {
  const RowTile({
    super.key,
    required this.index,
    required this.entry,
    required this.selected,
    required this.onTap,
  });

  final int index;
  final Entry entry;
  final ValueNotifier<int> selected;
  final VoidCallback onTap;

  @override
  State<RowTile> createState() => _RowTileState();
}

class _RowTileState extends State<RowTile> {
  late bool _sel = widget.selected.value == widget.index;

  @override
  void initState() {
    super.initState();
    widget.selected.addListener(_changed);
  }

  @override
  void didUpdateWidget(RowTile old) {
    super.didUpdateWidget(old);
    if (old.selected != widget.selected) {
      old.selected.removeListener(_changed);
      widget.selected.addListener(_changed);
    }
    _sel = widget.selected.value == widget.index;
  }

  @override
  void dispose() {
    widget.selected.removeListener(_changed);
    super.dispose();
  }

  void _changed() {
    final now = widget.selected.value == widget.index;
    if (now != _sel) setState(() => _sel = now);
  }

  @override
  Widget build(BuildContext context) {
    final p = PaletteScope.of(context);
    final tool = widget.entry.tool!;
    final nameStyle = sysStyle(13.5, FontWeight.w500, p.label);
    final spans = _nameSpans(tool.name, widget.entry.match, nameStyle);

    return GestureDetector(
      behavior: HitTestBehavior.opaque,
      onTap: widget.onTap,
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 20),
        child: Row(
          children: [
            IconTile(tool: tool),
            const SizedBox(width: 12),
            Expanded(
              child: Align(
                alignment: Alignment.centerLeft,
                child: Row(
                  mainAxisSize: MainAxisSize.min,
                  crossAxisAlignment: CrossAxisAlignment.baseline,
                  textBaseline: TextBaseline.alphabetic,
                  children: [
                    Text.rich(TextSpan(children: spans, style: nameStyle), maxLines: 1, softWrap: false),
                    const SizedBox(width: 8),
                    Flexible(
                      child: Text(
                        tool.subtitle,
                        maxLines: 1,
                        softWrap: false,
                        overflow: TextOverflow.ellipsis,
                        style: sysStyle(12, FontWeight.w400, p.secondary),
                      ),
                    ),
                  ],
                ),
              ),
            ),
            const SizedBox(width: 12),
            if (_sel)
              KeyCap('↵', fill: p.label.withValues(alpha: p.label.a * 0.12))
            else
              Text(tool.category, maxLines: 1, style: sysStyle(11, FontWeight.w400, p.tertiary)),
          ],
        ),
      ),
    );
  }
}

/// Matched characters render semibold.
List<TextSpan> _nameSpans(String name, List<int> match, TextStyle base) {
  if (match.isEmpty) return [TextSpan(text: name)];
  final bold = base.copyWith(fontWeight: FontWeight.w600);
  final hit = match.toSet();
  final out = <TextSpan>[];
  var i = 0;
  while (i < name.length) {
    final isHit = hit.contains(i);
    var j = i + 1;
    while (j < name.length && hit.contains(j) == isHit) {
      j++;
    }
    out.add(TextSpan(text: name.substring(i, j), style: isHit ? bold : null));
    i = j;
  }
  return out;
}

/// File chip at the trailing edge of the search row.
class FileChip extends StatelessWidget {
  const FileChip({super.key, required this.name, required this.onClear});
  final String name;
  final VoidCallback onClear;

  @override
  Widget build(BuildContext context) {
    final p = PaletteScope.of(context);
    final style = sysStyle(12, FontWeight.w500, p.label);
    // 200 max: 8 left + text + 4 gap + 14 x + 6 right.
    final label = middleTruncate(name, style, 200 - 8 - 4 - 14 - 6);
    return GestureDetector(
      behavior: HitTestBehavior.opaque,
      onTap: onClear,
      child: Container(
        height: 24,
        padding: const EdgeInsets.only(left: 8, right: 6),
        decoration: BoxDecoration(color: p.chipFill, borderRadius: BorderRadius.circular(8)),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            Text(label, maxLines: 1, softWrap: false, style: style),
            const SizedBox(width: 4),
            Icon(LucideIcons.x, size: 14, color: p.secondary),
          ],
        ),
      ),
    );
  }
}

String middleTruncate(String s, TextStyle style, double maxWidth) {
  final tp = TextPainter(textDirection: TextDirection.ltr, maxLines: 1);
  bool fits(String t) {
    tp.text = TextSpan(text: t, style: style);
    tp.layout();
    return tp.width <= maxWidth;
  }

  if (fits(s)) return s;
  for (var keep = s.length - 1; keep > 2; keep--) {
    final head = (keep + 1) ~/ 2;
    final tail = keep ~/ 2;
    final t = '${s.substring(0, head)}…${s.substring(s.length - tail)}';
    if (fits(t)) {
      tp.dispose();
      return t;
    }
  }
  tp.dispose();
  return '…';
}
