import 'package:flutter/widgets.dart';

/// Puts dictated [text] into the composer: at the cursor (replacing a selection), with a space on either side
/// where it would otherwise touch a word; at the end when the field has no cursor. The cursor lands after it.
void insertAtCursor(TextEditingController controller, String text) {
  final String insert = text.trim();
  if (insert.isEmpty) return;
  final String current = controller.text;
  final TextSelection selection = controller.selection;
  final int start = selection.isValid ? selection.start.clamp(0, current.length) : current.length;
  final int end = selection.isValid ? selection.end.clamp(0, current.length) : current.length;
  final String before = current.substring(0, start);
  final String after = current.substring(end);
  final bool spaceBefore = before.isNotEmpty && !RegExp(r'\s$').hasMatch(before);
  final bool spaceAfter = after.isNotEmpty && !RegExp(r'^\s').hasMatch(after);
  final String piece = '${spaceBefore ? ' ' : ''}$insert${spaceAfter ? ' ' : ''}';
  final int cursor = before.length + piece.length - (spaceAfter ? 1 : 0);
  controller.value = TextEditingValue(
    text: '$before$piece$after',
    selection: TextSelection.collapsed(offset: cursor),
  );
}
