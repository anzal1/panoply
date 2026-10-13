import 'package:flutter/widgets.dart';

/// Colors resolved for one appearance. Secondary and tertiary are 60% and 40%
/// of the label color, fills are a fraction of it, like SwiftUI's
/// `Color.primary.opacity(x)`.
class Palette {
  Palette._(this.dark)
      : label = dark ? const Color(0xD9FFFFFF) : const Color(0xD9000000),
        hairline = dark ? const Color(0x1FFFFFFF) : const Color(0x14000000);

  static final Palette light = Palette._(false);
  static final Palette darkPalette = Palette._(true);
  static Palette of(Brightness b) => b == Brightness.dark ? darkPalette : light;

  final bool dark;
  final Color label;

  /// 0.5 pt inner hairline: white 12% in dark, black 8% in light.
  final Color hairline;

  Color _scaled(double f) => label.withValues(alpha: label.a * f);

  Color get secondary => _scaled(0.60);
  Color get tertiary => _scaled(0.40);
  Color get separator => _scaled(0.08);
  Color get chipFill => _scaled(0.08);
  Color get keyCapFill => _scaled(0.08);
  Color get pillFill => _scaled(0.10);

  /// System colors (light, dark variants) for the category tints.
  Color tint(String category) {
    final t = _tints[category] ?? _tints['Files']!;
    return dark ? t.$2 : t.$1;
  }

  static const _tints = <String, (Color, Color)>{
    'PDF': (Color(0xFFFF3B30), Color(0xFFFF453A)), // red
    'Image': (Color(0xFFAF52DE), Color(0xFFBF5AF2)), // purple
    'Video': (Color(0xFFFF9500), Color(0xFFFF9F0A)), // orange
    'Audio': (Color(0xFFFF2D55), Color(0xFFFF375F)), // pink
    'Files': (Color(0xFF007AFF), Color(0xFF0A84FF)), // blue
    'Text': (Color(0xFF34C759), Color(0xFF30D158)), // green
    'Capture': (Color(0xFF30B0C7), Color(0xFF40C8E0)), // teal
  };
}

class PaletteScope extends InheritedWidget {
  const PaletteScope({super.key, required this.palette, required super.child});
  final Palette palette;

  static Palette of(BuildContext context) =>
      context.dependOnInheritedWidgetOfExactType<PaletteScope>()!.palette;

  @override
  bool updateShouldNotify(PaletteScope old) => old.palette != palette;
}

const String kFontFamily = '.AppleSystemUIFont';
const List<FontFeature> kTabular = [FontFeature.tabularFigures()];

TextStyle sysStyle(double size, FontWeight weight, Color color) => TextStyle(
      inherit: false,
      fontFamily: kFontFamily,
      fontSize: size,
      fontWeight: weight,
      color: color,
      fontFeatures: kTabular,
      decoration: TextDecoration.none,
      height: 1.2,
      leadingDistribution: TextLeadingDistribution.even,
    );
