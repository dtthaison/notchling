import 'package:agent_console/features/chat/markdown_text.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/harness.dart';

const String sample = '''
Đây là bảng:

| Bảng | Ý nghĩa | Khoá quan trọng |
|---|---|---|
| `users` | Người dùng | `id`, **email** |
| `orders` | Đơn hàng | `user_id` → `users.id` |

---

Hết.''';

void main() {
  test('the sample parses into a paragraph, a 3 column table, a rule and a paragraph', () {
    final List<MdBlock> blocks = parseProse(sample);
    expect(blocks.map((MdBlock b) => b.runtimeType), <Type>[MdParagraph, MdTable, MdRule, MdParagraph]);
    final MdTable table = blocks[1] as MdTable;
    expect(table.header, <String>['Bảng', 'Ý nghĩa', 'Khoá quan trọng']);
    expect(table.rows, hasLength(2));
    expect(table.rows[0], <String>['`users`', 'Người dùng', '`id`, **email**']);
  });

  test('alignment markers, escaped pipes and pipes inside code', () {
    final MdTable table = parseProse('| a | b | c |\n|:--|:-:|--:|\n| x\\|y | `p|q` | z |').single as MdTable;
    expect(table.aligns, <MdAlign>[MdAlign.left, MdAlign.center, MdAlign.right]);
    expect(table.rows.single, <String>['x|y', '`p|q`', 'z']);
  });

  test('thematic breaks: ---, ***, ___ and spaced', () {
    for (final String rule in <String>['---', '***', '___', '- - -', '----------']) {
      expect(parseProse('a\n\n$rule\n\nb').map((MdBlock b) => b.runtimeType), <Type>[MdParagraph, MdRule, MdParagraph], reason: rule);
    }
  });

  test('a table after a list item still parses, indented or not', () {
    final List<MdBlock> blocks = parseProse('- item\n  | a | b |\n  |---|---|\n  | 1 | 2 |');
    expect(blocks.map((MdBlock b) => b.runtimeType), <Type>[MdItem, MdTable]);
  });

  test('quotes merge, task lists, nested lists and headings', () {
    final List<MdBlock> blocks = parseProse('> one\n> two\n\n- [ ] todo\n- [x] done\n  - nested\n\n#### small');
    expect(blocks[0], isA<MdQuote>());
    expect((blocks[0] as MdQuote).text, 'one\ntwo');
    expect((blocks[1] as MdItem).checked, false);
    expect((blocks[2] as MdItem).checked, true);
    expect((blocks[3] as MdItem).depth, 1);
    expect((blocks[4] as MdHeading).level, 4);
  });

  testWidgets('the table renders as cells, not raw pipes, and the rule as a divider', (WidgetTester tester) async {
    await tester.pumpWidget(host(const MarkdownText(sample), screen: true == false));
    expect(find.byType(MarkdownTable), findsOneWidget);
    expect(find.textContaining('|---|'), findsNothing);
    expect(find.textContaining('---'), findsNothing);
    expect(find.text('Ý nghĩa', findRichText: true), findsOneWidget);
    expect(find.byType(Divider), findsOneWidget);
  });

  testWidgets('a long table scrolls sideways inside the bubble and columns never squash', (WidgetTester tester) async {
    tester.view.physicalSize = const Size(360 * 2, 800 * 2);
    tester.view.devicePixelRatio = 2;
    addTearDown(tester.view.reset);
    final String wide = '| ${List<String>.generate(8, (int i) => 'Cột số $i dài').join(' | ')} |\n|${List<String>.filled(8, '---').join('|')}|\n'
        '| ${List<String>.generate(8, (int i) => 'giá trị khá dài của ô $i để cần rộng').join(' | ')} |';
    await tester.pumpWidget(host(SizedBox(width: 320, child: MarkdownText(wide))));
    final Finder scroller = find.descendant(of: find.byType(MarkdownTable), matching: find.byType(SingleChildScrollView));
    expect(scroller, findsOneWidget);
    final ScrollableState state = tester.state(find.descendant(of: scroller, matching: find.byType(Scrollable)));
    expect(state.position.maxScrollExtent, greaterThan(100), reason: 'wider than the 320 dp bubble, so it scrolls');
    final double columnWidth = tester.getSize(find.descendant(of: find.byType(MarkdownTable), matching: find.byType(Table))).width / 8;
    expect(columnWidth, greaterThan(56), reason: 'columns keep their natural width');
    expect(columnWidth, lessThanOrEqualTo(240.5), reason: 'capped, the cell wraps');
  });
}
