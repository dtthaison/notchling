import 'package:flutter/material.dart';

import '../../app/theme/tokens.dart';

/// Basic markdown for chat: **bold**, `inline code` and ``` fenced code blocks (Geist Mono). Anything
/// else shows as written.
class MarkdownText extends StatefulWidget {
  const MarkdownText(this.text, {super.key});
  final String text;

  @override
  State<MarkdownText> createState() => _MarkdownTextState();
}

class _MarkdownTextState extends State<MarkdownText> {
  // Parsed once per text: the page rebuilds this widget often (keyboard, scrolling), the markdown does not change.
  late List<MarkdownSegment> _segments = splitSegments(widget.text);
  late final Map<int, List<InlineSpan>> _spans = <int, List<InlineSpan>>{};

  @override
  void didUpdateWidget(MarkdownText oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.text != widget.text) {
      _segments = splitSegments(widget.text);
      _spans.clear();
    }
  }

  @override
  Widget build(BuildContext context) {
    final List<Widget> children = <Widget>[];
    for (final MapEntry<int, MarkdownSegment> entry in _segments.asMap().entries) {
      final MarkdownSegment segment = entry.value;
      if (segment.isCode) {
        children.add(Container(
          width: double.infinity,
          padding: const EdgeInsets.all(Space.md),
          decoration: BoxDecoration(color: Token.neutral, borderRadius: BorderRadius.circular(Radii.md), border: Border.all(color: Token.border)),
          child: SingleChildScrollView(
            scrollDirection: Axis.horizontal,
            child: Text(segment.text, style: TypeToken.codeSm.copyWith(color: Token.onSurface)),
          ),
        ));
      } else {
        children.add(Text.rich(TextSpan(children: _spans.putIfAbsent(entry.key, () => inlineSpans(segment.text))), style: TypeToken.bodyMd));
      }
    }
    return Column(crossAxisAlignment: CrossAxisAlignment.start, children: <Widget>[
      for (int i = 0; i < children.length; i++) ...<Widget>[if (i > 0) const SizedBox(height: Space.sm), children[i]],
    ]);
  }
}

class MarkdownSegment {
  const MarkdownSegment(this.text, {required this.isCode});
  final String text;
  final bool isCode;
}

/// Splits on ``` fences. An unclosed fence runs to the end.
List<MarkdownSegment> splitSegments(String text) {
  final List<MarkdownSegment> result = <MarkdownSegment>[];
  final List<String> prose = <String>[];
  final List<String> code = <String>[];
  bool inCode = false;
  void flushProse() {
    final String joined = prose.join('\n').trim();
    if (joined.isNotEmpty) result.add(MarkdownSegment(joined, isCode: false));
    prose.clear();
  }

  for (final String line in text.split('\n')) {
    if (line.trim().startsWith('```')) {
      if (inCode) {
        result.add(MarkdownSegment(code.join('\n'), isCode: true));
        code.clear();
      } else {
        flushProse();
      }
      inCode = !inCode;
    } else if (inCode) {
      code.add(line);
    } else {
      prose.add(line);
    }
  }
  if (inCode) {
    result.add(MarkdownSegment(code.join('\n'), isCode: true));
  } else {
    flushProse();
  }
  return result;
}

final RegExp _inlinePattern = RegExp(r'(\*\*[^*\n]+\*\*)|(`[^`\n]+`)');

/// **bold** and `code` runs; the rest is plain.
List<InlineSpan> inlineSpans(String text) {
  final List<InlineSpan> spans = <InlineSpan>[];
  int cursor = 0;
  for (final RegExpMatch match in _inlinePattern.allMatches(text)) {
    if (match.start > cursor) spans.add(TextSpan(text: text.substring(cursor, match.start)));
    final String token = match.group(0)!;
    if (token.startsWith('**')) {
      spans.add(TextSpan(text: token.substring(2, token.length - 2), style: TypeToken.bodyMdStrong));
    } else {
      spans.add(TextSpan(
          text: token.substring(1, token.length - 1), style: TypeToken.codeSm.copyWith(color: Token.onSurface, backgroundColor: Token.neutral)));
    }
    cursor = match.end;
  }
  if (cursor < text.length) spans.add(TextSpan(text: text.substring(cursor)));
  return spans;
}
