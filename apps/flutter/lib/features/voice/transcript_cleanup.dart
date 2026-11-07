/// Cleans what Whisper returns before it reaches the composer: hallucinated boilerplate, runs of repeated
/// characters, words and phrases, stray CJK and bracket tags.
String cleanTranscript(String raw) {
  String text = raw;
  text = text.replaceAll(RegExp(r'\[[^\]]*\]'), ' ');
  text = text.replaceAll(RegExp(r'[぀-ヿ㐀-鿿가-힯]+'), ' ');
  text = stripBoilerplate(text);
  text = collapseRepeats(text);
  return text.replaceAll(RegExp(r'\s+'), ' ').trim();
}

final List<RegExp> _boilerplate = <RegExp>[
  RegExp(r'hãy\s+(?:subscribe|đăng\s*ký)[^.!?\n]*[.!?]?', caseSensitive: false),
  RegExp(r'(?:đừng\s+quên\s+)?(?:subscribe|đăng\s*ký)\s+(?:cho\s+)?kênh[^.!?\n]*[.!?]?', caseSensitive: false),
  RegExp(r'ghiền\s+mì\s+gõ[^.!?\n]*[.!?]?', caseSensitive: false),
  RegExp(r'cảm\s+ơn\s+(?:các\s+bạn|quý\s+vị|mọi\s+người)\s+đã\s+(?:xem|theo\s+dõi|lắng\s+nghe)[^.!?\n]*[.!?]?', caseSensitive: false),
  RegExp(r'hẹn\s+gặp\s+lại\s+các\s+bạn[^.!?\n]*[.!?]?', caseSensitive: false),
  RegExp(r'thanks\s+for\s+watching[^.!?\n]*[.!?]?', caseSensitive: false),
  RegExp(r'subtitles?\s+by[^.!?\n]*[.!?]?', caseSensitive: false),
  RegExp(r'(?:video\s+)?phụ\s+đề\s+(?:bởi|của)[^.!?\n]*[.!?]?', caseSensitive: false),
];

String stripBoilerplate(String text) {
  String result = text;
  for (final RegExp pattern in _boilerplate) {
    result = result.replaceAll(pattern, ' ');
  }
  return result;
}

/// "OVVVV" -> "OV"; "đúng đúng đúng đúng" -> "đúng"; "chạy test chạy test chạy test" -> "chạy test".
/// A word said twice stays twice (people do say "bug bug"); three or more times in a row is a loop.
String collapseRepeats(String text) {
  // Four or more of the same character in a row.
  final String result = text.replaceAllMapped(RegExp(r'(\S)\1{3,}'), (Match m) => m.group(1)!);
  final List<String> words = result.split(RegExp(r'\s+')).where((String w) => w.isNotEmpty).toList();
  bool changed = true;
  while (changed) {
    changed = false;
    for (int size = 1; size <= 6 && !changed; size++) {
      for (int start = 0; start + size * 3 <= words.length && !changed; start++) {
        int repeats = 1;
        while (start + size * (repeats + 1) <= words.length && _sameRun(words, start, start + size * repeats, size)) {
          repeats++;
        }
        if (repeats >= 3) {
          words.removeRange(start + size, start + size * repeats);
          changed = true;
        }
      }
    }
  }
  return words.join(' ');
}

bool _sameRun(List<String> words, int a, int b, int size) {
  for (int i = 0; i < size; i++) {
    if (_norm(words[a + i]) != _norm(words[b + i])) return false;
  }
  return true;
}

String _norm(String word) => word.toLowerCase().replaceAll(RegExp(r'[^\p{L}\p{N}]', unicode: true), '');
