import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../../app/theme/tokens.dart';
import '../../l10n/app_localizations.dart';
import '../../app/widgets/kit.dart';
import '../../app/widgets/motion.dart';
import 'code_highlight.dart';

class MarkdownText extends StatefulWidget {
  const MarkdownText(this.text, {super.key});
  final String text;

  @override
  State<MarkdownText> createState() => _MarkdownTextState();
}

class _MarkdownTextState extends State<MarkdownText> {
  late List<MarkdownSegment> _segments = splitSegments(widget.text);
  late List<MdBlock> _blocks = _parseSegments(_segments);

  Kit? _builtFor;
  List<Widget> _built = const <Widget>[];

  @override
  void didUpdateWidget(MarkdownText oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.text != widget.text) {
      _segments = splitSegments(widget.text);
      _blocks = _parseSegments(_segments);
      _builtFor = null;
    }
  }

  @override
  Widget build(BuildContext context) {
    final Kit kit = Kit.of(context);
    if (!identical(_builtFor, kit)) {
      _builtFor = kit;
      _built = <Widget>[for (final MdBlock block in _blocks) _buildBlock(kit, block)];
    }
    final List<Widget> children = _built;
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

sealed class MdBlock {
  const MdBlock();
}

class MdCode extends MdBlock {
  const MdCode(this.code, this.language);
  final String code;
  final String? language;
}

class MdParagraph extends MdBlock {
  const MdParagraph(this.text);
  final String text;
}

class MdHeading extends MdBlock {
  const MdHeading(this.level, this.text);
  final int level;
  final String text;
}

class MdItem extends MdBlock {
  const MdItem({required this.marker, required this.text, required this.depth, this.checked, this.ordered = false});
  final String marker;

  final bool ordered;
  final String text;
  final int depth;

  final bool? checked;
}

class MdQuote extends MdBlock {
  const MdQuote(this.text);
  final String text;
}

class MdRule extends MdBlock {
  const MdRule();
}

enum MdAlign { left, center, right }

class MdTable extends MdBlock {
  const MdTable({required this.header, required this.aligns, required this.rows});
  final List<String> header;
  final List<MdAlign> aligns;
  final List<List<String>> rows;
}

final RegExp _headingRe = RegExp(r'^(#{1,4})\s+(.*)$');
final RegExp _taskRe = RegExp(r'^\[([ xX])\]\s+(.*)$');
final RegExp _bulletRe = RegExp(r'^(\s*)[-*•+]\s+(.*)$');
final RegExp _numberedRe = RegExp(r'^(\s*)(\d+)[.)]\s+(.*)$');
final RegExp _quoteRe = RegExp(r'^\s*>\s?(.*)$');
final RegExp _ruleRe = RegExp(r'^\s{0,3}([-*_])(\s*\1){2,}\s*$');
final RegExp _separatorCellRe = RegExp(r'^:?-+:?$');

List<String> splitTableRow(String line) {
  String row = line.trim();
  if (row.startsWith('|')) row = row.substring(1);
  final List<String> cells = <String>[];
  final StringBuffer cell = StringBuffer();
  bool inCode = false;
  for (int i = 0; i < row.length; i++) {
    final String char = row[i];
    if (char == r'\' && i + 1 < row.length && row[i + 1] == '|') {
      cell.write('|');
      i++;
    } else if (char == '`') {
      inCode = !inCode;
      cell.write(char);
    } else if (char == '|' && !inCode) {
      cells.add(cell.toString().trim());
      cell.clear();
    } else {
      cell.write(char);
    }
  }
  final String last = cell.toString().trim();
  if (last.isNotEmpty || !row.endsWith('|')) cells.add(last);
  return cells;
}

List<MdAlign>? _separatorAligns(String line) {
  if (!line.contains('-') || !(line.contains('|') || line.trim().startsWith(':'))) return null;
  final List<String> cells = splitTableRow(line);
  if (cells.isEmpty || !cells.every(_separatorCellRe.hasMatch)) return null;
  return <MdAlign>[
    for (final String cell in cells)
      if (cell.startsWith(':') && cell.endsWith(':'))
        MdAlign.center
      else if (cell.endsWith(':'))
        MdAlign.right
      else
        MdAlign.left,
  ];
}

List<MdBlock> _parseSegments(List<MarkdownSegment> segments) {
  final List<MdBlock> blocks = <MdBlock>[];
  for (final MarkdownSegment segment in segments) {
    if (segment.isCode) {
      blocks.add(MdCode(segment.text, segment.language));
    } else {
      blocks.addAll(parseProse(segment.text));
    }
  }
  return blocks;
}

List<MdBlock> parseProse(String text) {
  final List<MdBlock> blocks = <MdBlock>[];
  final List<String> paragraph = <String>[];
  final List<String> quote = <String>[];
  void flushParagraph() {
    if (paragraph.isEmpty) return;
    blocks.add(MdParagraph(paragraph.join('\n')));
    paragraph.clear();
  }

  void flushQuote() {
    if (quote.isEmpty) return;
    blocks.add(MdQuote(quote.join('\n')));
    quote.clear();
  }

  final List<String> lines = text.split('\n');
  for (int index = 0; index < lines.length; index++) {
    final String line = lines[index];
    final RegExpMatch? quoted = _quoteRe.firstMatch(line);
    if (quoted != null) {
      flushParagraph();
      quote.add(quoted.group(1)!);
      continue;
    }
    flushQuote();
    if (line.trim().isEmpty) {
      flushParagraph();
      continue;
    }
    if (_ruleRe.hasMatch(line)) {
      flushParagraph();
      blocks.add(const MdRule());
      continue;
    }
    if (line.contains('|') && index + 1 < lines.length) {
      final List<MdAlign>? aligns = _separatorAligns(lines[index + 1]);
      final List<String> header = splitTableRow(line);
      if (aligns != null && header.length == aligns.length) {
        flushParagraph();
        final List<List<String>> rows = <List<String>>[];
        int next = index + 2;
        while (next < lines.length && lines[next].trim().isNotEmpty && lines[next].contains('|')) {
          final List<String> cells = splitTableRow(lines[next]);
          rows.add(<String>[for (int c = 0; c < header.length; c++) c < cells.length ? cells[c] : '']);
          next++;
        }
        blocks.add(MdTable(header: header, aligns: aligns, rows: rows));
        index = next - 1;
        continue;
      }
    }
    final RegExpMatch? heading = _headingRe.firstMatch(line);
    if (heading != null) {
      flushParagraph();
      blocks.add(MdHeading(heading.group(1)!.length, heading.group(2)!));
      continue;
    }
    final RegExpMatch? bullet = _bulletRe.firstMatch(line);
    final RegExpMatch? numbered = _numberedRe.firstMatch(line);
    if (bullet != null || numbered != null) {
      flushParagraph();
      final String indent = (bullet?.group(1) ?? numbered!.group(1))!;
      final int depth = (indent.replaceAll('\t', '    ').length ~/ 2).clamp(0, 4);
      String body = bullet != null ? bullet.group(2)! : numbered!.group(3)!;
      bool? checked;
      final RegExpMatch? task = bullet != null ? _taskRe.firstMatch(body) : null;
      if (task != null) {
        checked = task.group(1) != ' ';
        body = task.group(2)!;
      }
      blocks.add(MdItem(
        marker: bullet != null ? '' : '${numbered!.group(2)}.',
        ordered: bullet == null,
        text: body,
        depth: depth,
        checked: checked,
      ));
      continue;
    }
    paragraph.add(line);
  }
  flushParagraph();
  flushQuote();
  return blocks;
}

Widget _buildBlock(Kit kit, MdBlock block) {
  switch (block) {
    case MdCode(:final String code, :final String? language):
      return CodeBlock(code: code, language: language);
    case MdParagraph(:final String text):
      return Text.rich(TextSpan(children: inlineSpans(text, kit)), style: kit.body);
    case MdHeading(:final int level, :final String text):
      return Padding(
        padding: const EdgeInsets.only(top: Space.xs),
        child: Text.rich(
          TextSpan(children: inlineSpans(text, kit)),
          style: (level == 1 ? kit.title : kit.heading).copyWith(fontSize: const <double>[19, 17, 15, 14][level - 1]),
        ),
      );
    case MdItem():
      return Padding(
        padding: EdgeInsets.only(left: 16.0 * block.depth),
        child: Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: <Widget>[
            SizedBox(
              width: 22,
              child: block.checked == null
                  ? (block.ordered
                        ? Text(block.marker, style: kit.body.copyWith(color: kit.textMuted))
                        : _BulletDot(depth: block.depth, color: kit.textMuted))
                  : Padding(
                      padding: const EdgeInsets.only(top: 2),
                      child: Icon(
                        block.checked! ? Icons.check_box : Icons.check_box_outline_blank,
                        size: 16,
                        color: block.checked! ? kit.accent : kit.textMuted,
                      ),
                    ),
            ),
            Expanded(
              child: Text.rich(
                TextSpan(children: inlineSpans(block.text, kit)),
                style: block.checked == true ? kit.body.copyWith(color: kit.textMuted) : kit.body,
              ),
            ),
          ],
        ),
      );
    case MdQuote(:final String text):
      return Container(
        padding: const EdgeInsets.only(left: Space.md),
        decoration: BoxDecoration(border: Border(left: BorderSide(color: kit.hairline, width: 2))),
        child: Text.rich(TextSpan(children: inlineSpans(text, kit)), style: kit.body.copyWith(color: kit.textMuted)),
      );
    case MdRule():
      return Padding(
        padding: const EdgeInsets.symmetric(vertical: Space.xs),
        child: Divider(height: 1, thickness: 1, color: kit.hairline),
      );
    case MdTable():
      return MarkdownTable(table: block);
  }
}

class _BulletDot extends StatelessWidget {
  const _BulletDot({required this.depth, required this.color});
  final int depth;
  final Color color;

  @override
  Widget build(BuildContext context) {
    final double size = depth == 0 ? 6 : 5;
    return Padding(
      padding: const EdgeInsets.only(top: 9, left: 4),
      child: Align(
        alignment: Alignment.topLeft,
        child: Container(
          width: size,
          height: size,
          decoration: BoxDecoration(
            shape: BoxShape.circle,
            color: depth % 2 == 0 ? color : null,
            border: depth % 2 == 1 ? Border.all(color: color, width: 1.2) : null,
          ),
        ),
      ),
    );
  }
}

const double _maxCellWidth = 240;
const double _minCellWidth = 56;
const EdgeInsets _cellPadding = EdgeInsets.symmetric(horizontal: 10, vertical: 8);

String _plain(String markdown) => markdown
    .replaceAllMapped(RegExp(r'\[([^\]\n]+)\]\([^)\s]+\)'), (Match m) => m.group(1)!)
    .replaceAll(RegExp(r'[*`~]'), '');

class MarkdownTable extends StatefulWidget {
  const MarkdownTable({super.key, required this.table});
  final MdTable table;

  @override
  State<MarkdownTable> createState() => _MarkdownTableState();
}

class _MarkdownTableState extends State<MarkdownTable> {
  final ScrollController _scroll = ScrollController();
  bool _more = true;
  Kit? _measuredFor;
  late List<double> _widths;

  @override
  void initState() {
    super.initState();
    _scroll.addListener(_onScroll);
  }

  @override
  void dispose() {
    _scroll.dispose();
    super.dispose();
  }

  void _onScroll() {
    final bool more = _scroll.hasClients && _scroll.offset < _scroll.position.maxScrollExtent - 1;
    if (more != _more) setState(() => _more = more);
  }

  List<double> _measure(Kit kit, MediaQueryData media) {
    final MdTable table = widget.table;
    final List<double> widths = <double>[];
    for (int column = 0; column < table.header.length; column++) {
      double widest = _minCellWidth;
      final List<(String, TextStyle)> cells = <(String, TextStyle)>[
        (table.header[column], kit.body.copyWith(fontWeight: FontWeight.w600)),
        for (final List<String> row in table.rows) (row[column], kit.body),
      ];
      for (final (String, TextStyle) cell in cells) {
        final TextPainter painter = TextPainter(
          text: TextSpan(text: _plain(cell.$1), style: cell.$2),
          textDirection: TextDirection.ltr,
          textScaler: media.textScaler,
        )..layout();
        final double needed = painter.width + _cellPadding.horizontal;
        if (needed > widest) widest = needed;
        painter.dispose();
      }
      widths.add(widest > _maxCellWidth ? _maxCellWidth : widest);
    }
    return widths;
  }

  TextAlign _textAlign(MdAlign align) => switch (align) {
    MdAlign.left => TextAlign.left,
    MdAlign.center => TextAlign.center,
    MdAlign.right => TextAlign.right,
  };

  @override
  Widget build(BuildContext context) {
    final Kit kit = Kit.of(context);
    final MdTable table = widget.table;
    if (!identical(_measuredFor, kit)) {
      _measuredFor = kit;
      _widths = _measure(kit, MediaQuery.of(context));
    }
    final double natural = _widths.fold<double>(0, (double a, double b) => a + b);
    return LayoutBuilder(
      builder: (BuildContext context, BoxConstraints box) {
        final double scale = natural < box.maxWidth ? box.maxWidth / natural : 1;
        final bool overflows = natural > box.maxWidth + 0.5;
        Widget cell(String text, int column, {required bool head}) => Padding(
          padding: _cellPadding,
          child: Text.rich(
            TextSpan(children: inlineSpans(text, kit)),
            textAlign: _textAlign(table.aligns[column]),
            style: head ? kit.body.copyWith(color: kit.textMuted, fontWeight: FontWeight.w600) : kit.body,
          ),
        );
        final Widget grid = Table(
          columnWidths: <int, TableColumnWidth>{
            for (int c = 0; c < _widths.length; c++) c: FixedColumnWidth(_widths[c] * scale),
          },
          defaultVerticalAlignment: TableCellVerticalAlignment.top,
          border: TableBorder(horizontalInside: BorderSide(color: kit.hairline)),
          children: <TableRow>[
            TableRow(children: <Widget>[for (int c = 0; c < table.header.length; c++) cell(table.header[c], c, head: true)]),
            for (final List<String> row in table.rows)
              TableRow(children: <Widget>[for (int c = 0; c < row.length; c++) cell(row[c], c, head: false)]),
          ],
        );
        return DecoratedBox(
          decoration: kitCard(kit, radius: Radii.md, fill: kit.raised),
          child: ClipRRect(
            borderRadius: BorderRadius.circular(Radii.md),
            child: Stack(
              children: <Widget>[
                SingleChildScrollView(controller: _scroll, scrollDirection: Axis.horizontal, child: grid),
                if (overflows && _more)
                  Positioned(
                    top: 0,
                    bottom: 0,
                    right: 0,
                    width: 28,
                    child: IgnorePointer(
                      child: DecoratedBox(
                        decoration: BoxDecoration(
                          gradient: LinearGradient(colors: <Color>[kit.raised.withValues(alpha: 0), kit.raised]),
                        ),
                      ),
                    ),
                  ),
              ],
            ),
          ),
        );
      },
    );
  }
}

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
                      _copied ? l10n.chatCopied : l10n.chatCopyCode,
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
final RegExp _strikeRe = RegExp(r'~~(?=\S)(.+?)(?<=\S)~~');
final RegExp _linkRe = RegExp(r'\[([^\]\n]+)\]\(([^)\s]+)\)');

const int _boxedCodeLimit = 28;

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
      _strikeRe,
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
      case 4:
        spans.addAll(
          inlineSpans(
            inner,
            kit,
            style: (style ?? const TextStyle()).copyWith(
              decoration: TextDecoration.lineThrough,
              color: kit.textMuted,
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
