import 'dart:convert';

import 'package:flutter/services.dart' show rootBundle;

class Tool {
  const Tool({
    required this.id,
    required this.name,
    required this.subtitle,
    required this.category,
    required this.lucide,
    required this.accepts,
  });

  final String id;
  final String name;
  final String subtitle;
  final String category;
  final String lucide;
  final List<String> accepts;

  factory Tool.fromJson(Map<String, dynamic> j) => Tool(
        id: j['id'] as String,
        name: j['name'] as String,
        subtitle: j['subtitle'] as String,
        category: j['category'] as String,
        lucide: j['lucide'] as String,
        accepts: (j['accepts'] as List).cast<String>(),
      );

  bool acceptsExt(String ext) => accepts.contains('*') || accepts.contains(ext);
}

/// Loads the shared tool list (bundled as assets/utilities.json).
Future<List<Tool>> loadTools() async {
  final raw = await rootBundle.loadString('assets/utilities.json');
  return [for (final e in jsonDecode(raw) as List) Tool.fromJson(e as Map<String, dynamic>)];
}

/// One line of the results list: a section header or a tool row.
class Entry {
  const Entry.header(this.title, this.top)
      : tool = null,
        match = const [];
  const Entry.row(Tool this.tool, this.match, this.top) : title = null;

  final String? title;
  final Tool? tool;
  final List<int> match;
  final double top;

  bool get isHeader => tool == null;
  double get height => isHeader ? kHeaderHeight : kRowHeight;
  double get bottom => top + height;
}

const double kHeaderHeight = 28;
const double kRowHeight = 44;
const double kListTopPad = 8;
const double kListBottomPad = 8;
