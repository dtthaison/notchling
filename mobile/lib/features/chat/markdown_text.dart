import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../../app/theme/tokens.dart';
import '../../l10n/app_localizations.dart';
import '../../app/widgets/kit.dart';
import '../../app/widgets/motion.dart';
import 'code_highlight.dart';

/// Markdown for chat: headings, paragraphs, bullet and numbered lists, quotes, **bold**, *italic*, `inline code`,
/// links and fenced code blocks (highlighted, with a copy button). Anything else shows as written.
class MarkdownText extends StatefulWidget {
  const MarkdownText(this.text, {super.key});
  final String text;

  @override
  State<MarkdownText> createState() => _MarkdownTextState();
}

class _MarkdownTextState extends State<MarkdownText> {
  // Parsed once per text: the page rebuilds this widget often (keyboard, scrolling), the markdown does not change.
  late List<MarkdownSegment> _segments = splitSegments(widget.text);

  @override
  void didUpdateWidget(MarkdownText oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.text != widget.text) _segments = splitSegments(widget.text);
  }

  @override
  Widget build(BuildContext context) {
    final Kit kit = Kit.of(context);
    final List<Widget> children = <Widget>[];
    for (final MarkdownSegment segment in _segments) {
      if (segment.isCode) {
        children.add(CodeBlock(code: segment.text, language: segment.language));
      } else {
        children.addAll(_proseBlocks(context, kit, segment.text));
      }
    }
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: <Widget>[
        for (int i = 0; i < children.length; i++) ...<Widget>[
          if (i > 0) const SizedBox(height: Space.sm),
          children[i],
        ],
      ],
    );
  }
}

List<Widget> _proseBlocks(BuildContext context, Kit kit, String text) {
  final List<Widget> blocks = <Widget>[];
  final List<String> paragraph = <String>[];
  void flush() {
    if (paragraph.isEmpty) return;
    blocks.add(
      Text.rich(
        TextSpan(children: inlineSpans(paragraph.join('\n'), kit)),
        style: kit.body,
      ),
    );
    paragraph.clear();
  }

  for (final String line in text.split('\n')) {
    final RegExpMatch? heading = RegExp(r'^(#{1,4})\s+(.*)$').firstMatch(line);
    final RegExpMatch? bullet = RegExp(r'^\s*[-*•]\s+(.*)$').firstMatch(line);
    final RegExpMatch? numbered = RegExp(
      r'^\s*(\d+)[.)]\s+(.*)$',
    ).firstMatch(line);
    final RegExpMatch? quote = RegExp(r'^\s*>\s?(.*)$').firstMatch(line);
    if (line.trim().isEmpty) {
      flush();
    } else if (heading != null) {
      flush();
      final int level = heading.group(1)!.length;
      blocks.add(
        Padding(
          padding: const EdgeInsets.only(top: Space.xs),
          child: Text.rich(
            TextSpan(children: inlineSpans(heading.group(2)!, kit)),
            style: (level == 1 ? kit.title : kit.heading).copyWith(
              fontSize: level == 1 ? 19 : (level == 2 ? 17 : 15),
            ),
          ),
        ),
      );
    } else if (bullet != null || numbered != null) {
      flush();
      final String marker = bullet != null ? '•' : '${numbered!.group(1)}.';
      final String body = bullet != null
          ? bullet.group(1)!
          : numbered!.group(2)!;
      blocks.add(
        Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: <Widget>[
            SizedBox(
              width: 22,
              child: Text(
                marker,
                style: kit.body.copyWith(color: kit.textMuted),
              ),
            ),
            Expanded(
              child: Text.rich(
                TextSpan(children: inlineSpans(body, kit)),
                style: kit.body,
              ),
            ),
          ],
        ),
      );
    } else if (quote != null) {
      flush();
      blocks.add(
        Container(
          padding: const EdgeInsets.only(left: Space.md),
          decoration: BoxDecoration(
            border: Border(left: BorderSide(color: kit.hairline, width: 2)),
          ),
          child: Text.rich(
            TextSpan(children: inlineSpans(quote.group(1)!, kit)),
            style: kit.body.copyWith(color: kit.textMuted),
          ),
        ),
      );
    } else {
      paragraph.add(line);
    }
  }
  flush();
  return blocks;
}

/// A fenced code block: raised box, language label, copy button, highlighted, scrolls sideways.
class CodeBlock extends StatefulWidget {
  const CodeBlock({super.key, required this.code, this.language});
  final String code;
  final String? language;

  @override
  State<CodeBlock> createState() => _CodeBlockState();
}

class _CodeBlockState extends State<CodeBlock> {
  bool _copied = false;
  Timer? _reset;

  void _copy() {
    unawaited(Clipboard.setData(ClipboardData(text: widget.code)));
    setState(() => _copied = true);
    _reset?.cancel();
    _reset = Timer(const Duration(milliseconds: 1600), () {
      if (mounted) setState(() => _copied = false);
    });
  }

  @override
  void dispose() {
    _reset?.cancel();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final String code = widget.code;
    final String? language = widget.language;
    final Kit kit = Kit.of(context);
    final AppL10n l10n = AppL10n.of(context);
    final String? lang = languageOf(language);
    return Container(
      width: double.infinity,
      decoration: kitCard(kit, radius: Radii.sm, fill: kit.raised),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          Padding(
            padding: const EdgeInsets.fromLTRB(Space.md, Space.xs, Space.xs, 0),
            child: Row(
              children: <Widget>[
                Expanded(child: Text(lang ?? '', style: kit.meta)),
                SizedBox(
                  height: 32,
                  child: TextButton.icon(
                    onPressed: _copy,
                    icon: AnimatedSwitcher(
                      duration: motionDuration(context, Motion.quick),
                      child: Icon(
                        _copied ? Icons.check : Icons.copy,
                        key: ValueKey<bool>(_copied),
                        size: 14,
                        color: _copied ? kit.done : kit.textMuted,
                      ),
                    ),
                    label: Text(
                      _copied ? l10n.copied : l10n.copyCode,
                      style: kit.meta.copyWith(
                        color: _copied ? kit.done : null,
                      ),
                    ),
                    style: TextButton.styleFrom(
                      minimumSize: const Size(48, 32),
                      padding: const EdgeInsets.symmetric(horizontal: Space.sm),
                    ),
                  ),
                ),
              ],
            ),
          ),
          SingleChildScrollView(
            scrollDirection: Axis.horizontal,
            padding: const EdgeInsets.fromLTRB(Space.md, 0, Space.md, Space.md),
            child: Text.rich(
              TextSpan(children: highlightCode(code, kit, language: lang)),
              style: kit.mono,
            ),
          ),
        ],
      ),
    );
  }
}

class MarkdownSegment {
  const MarkdownSegment(this.text, {required this.isCode, this.language});
  final String text;
  final bool isCode;
  final String? language;
}

/// Splits on ``` fences. An unclosed fence runs to the end.
List<MarkdownSegment> splitSegments(String text) {
  final List<MarkdownSegment> result = <MarkdownSegment>[];
  final List<String> prose = <String>[];
  final List<String> code = <String>[];
  String? language;
  bool inCode = false;
  void flushProse() {
    final String joined = prose.join('\n').trim();
    if (joined.isNotEmpty) result.add(MarkdownSegment(joined, isCode: false));
    prose.clear();
  }

  for (final String line in text.split('\n')) {
    if (line.trim().startsWith('```')) {
      if (inCode) {
        result.add(
          MarkdownSegment(code.join('\n'), isCode: true, language: language),
        );
        code.clear();
      } else {
        flushProse();
        final String info = line.trim().substring(3).trim();
        language = info.isEmpty ? null : info.split(RegExp(r'\s+')).first;
      }
      inCode = !inCode;
    } else if (inCode) {
      code.add(line);
    } else {
      prose.add(line);
    }
  }
  if (inCode) {
    result.add(
      MarkdownSegment(code.join('\n'), isCode: true, language: language),
    );
  } else {
    flushProse();
  }
  return result;
}

final RegExp _codeRe = RegExp(r'`([^`\n]+)`');
final RegExp _boldRe = RegExp(r'\*\*(?=\S)(.+?)(?<=\S)\*\*');
final RegExp _italicRe = RegExp(
  r'(?<![*\w])\*(?=[^\s*])([^*\n]+?)(?<=[^\s*])\*(?![*\w])',
);
final RegExp _linkRe = RegExp(r'\[([^\]\n]+)\]\(([^)\s]+)\)');

/// Inline code up to this many characters is drawn as a small box aligned to the text baseline; longer runs wrap
/// like text, with a background.
const int _boxedCodeLimit = 28;

/// **bold**, *italic*, `code` and [links](url) runs; the rest is plain. Runs nest: `code`, links and italics inside
/// **bold**, bold inside italics. Code is never parsed further.
List<InlineSpan> inlineSpans(String text, Kit kit, {TextStyle? style}) {
  final List<InlineSpan> spans = <InlineSpan>[];
  int cursor = 0;
  while (cursor < text.length) {
    RegExpMatch? best;
    int kind = -1;
    final List<RegExp> patterns = <RegExp>[
      _codeRe,
      _boldRe,
      _linkRe,
      _italicRe,
    ];
    for (int k = 0; k < patterns.length; k++) {
      final RegExpMatch? match = patterns[k].firstMatch(text.substring(cursor));
      if (match != null && (best == null || match.start < best.start)) {
        best = match;
        kind = k;
      }
    }
    if (best == null) {
      spans.add(TextSpan(text: text.substring(cursor), style: style));
      break;
    }
    if (best.start > 0) {
      spans.add(
        TextSpan(
          text: text.substring(cursor, cursor + best.start),
          style: style,
        ),
      );
    }
    final String inner = best.group(1)!;
    switch (kind) {
      case 0:
        spans.add(_codeSpan(inner, kit, style));
      case 1:
        spans.addAll(
          inlineSpans(
            inner,
            kit,
            style: (style ?? const TextStyle()).copyWith(
              fontWeight: FontWeight.w600,
            ),
          ),
        );
      case 2:
        spans.addAll(
          inlineSpans(
            inner,
            kit,
            style: (style ?? const TextStyle()).copyWith(
              color: kit.accentText,
              decoration: TextDecoration.underline,
            ),
          ),
        );
      default:
        spans.addAll(
          inlineSpans(
            inner,
            kit,
            style: (style ?? const TextStyle()).copyWith(
              fontStyle: FontStyle.italic,
            ),
          ),
        );
    }
    cursor += best.end;
  }
  return spans;
}

InlineSpan _codeSpan(String code, Kit kit, TextStyle? outer) {
  final TextStyle mono = kit.mono.copyWith(
    fontSize: 13,
    height: 1.2,
    color: outer?.color ?? kit.text,
  );
  if (code.length > _boxedCodeLimit) {
    return TextSpan(
      text: code,
      style: mono.copyWith(backgroundColor: kit.raised, height: null),
    );
  }
  return WidgetSpan(
    alignment: PlaceholderAlignment.baseline,
    baseline: TextBaseline.alphabetic,
    child: Container(
      margin: const EdgeInsets.symmetric(horizontal: 1),
      padding: const EdgeInsets.symmetric(horizontal: 4),
      decoration: BoxDecoration(
        color: kit.raised,
        borderRadius: BorderRadius.circular(Radii.sm - 2),
      ),
      child: Text(code, style: mono, softWrap: false),
    ),
  );
}
