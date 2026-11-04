/// A reply starting with "!" runs as a shell command in the session, as in Claude Code's prompt. Commands are often
/// copied from Claude's answer in a terminal, which wraps long lines and indents each continuation; pasted as is,
/// the shell would run the pieces separately. Same rules as the Mac's `ShellCommand` (Sources/Notchling/Agents).
abstract final class ShellCommand {
  /// Sending trims leading spaces, so "  !ls" runs too.
  static bool isCommand(String text) => text.trimLeft().startsWith('!');

  /// The command as it should run, still starting with "!". Text that is not a command comes back as is.
  /// Running it twice changes nothing.
  static String normalize(String text) {
    if (!isCommand(text)) return text;
    String body = text.trimLeft().substring(1);
    body = _plain(body.replaceAll('\r\n', '\n'));
    final List<String> lines = body
        .split('\n')
        .where((String l) => !l.trim().startsWith('```'))
        .toList();
    final bool joinWrapped = !body.contains('<<');
    String result = '';
    for (int index = 0; index < lines.length; index++) {
      final String raw = lines[index];
      if (index == 0 || result.isEmpty) {
        result = _withoutPrompt(raw.trimLeft());
        continue;
      }
      final bool continues =
          raw.isNotEmpty && raw[0].trim().isEmpty && raw.trim().isNotEmpty;
      if (joinWrapped && continues && !result.endsWith(r'\')) {
        final String rest = raw.trimLeft();
        if (result.isNotEmpty && result[result.length - 1].trim().isEmpty) {
          result += rest;
        } else if (_endsWithSeparator(result)) {
          result += ' $rest';
        } else {
          result += rest;
        }
      } else {
        result += '\n${_withoutPrompt(raw)}';
      }
    }
    final String tidy = result
        .split('\n')
        .map((String l) => l.replaceAll(RegExp(r'[ \t]+$'), ''))
        .join('\n')
        .trim();
    return '!$tidy';
  }

  static String _plain(String s) {
    String out = s;
    const Map<String, String> map = <String, String>{
      '“': '"',
      '”': '"',
      '„': '"',
      '‘': "'",
      '’': "'",
      '—': '--',
      '–': '--',
      ' ': ' ',
      ' ': ' ',
      ' ': ' ',
      '​': '',
    };
    map.forEach((String from, String to) => out = out.replaceAll(from, to));
    return out;
  }

  static String _withoutPrompt(String line) {
    for (final String prompt in const <String>[r'$ ', '% ']) {
      if (line.startsWith(prompt)) return line.substring(prompt.length);
    }
    return line;
  }

  static bool _endsWithSeparator(String text) {
    final List<String> words = text
        .split(RegExp(r'\s+'))
        .where((String w) => w.isNotEmpty)
        .toList();
    return words.isNotEmpty &&
        const <String>[
          'then',
          'do',
          'else',
          '{',
          '(',
          '&&',
          '||',
          '|',
          ';',
        ].contains(words.last);
  }
}
