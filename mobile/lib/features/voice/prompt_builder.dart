/// The words Whisper is nudged toward: Vietnamese speech with English dev terms mixed in. In the P0b
/// comparison this prompt took Whisper small from 79 % to 45 % word error and from 9 % to 43 % correct terms.
const List<String> devGlossary = <String>[
  'commit', 'push', 'pull request', 'merge', 'branch', 'main', 'flutter', 'analyze', 'build', 'test', 'simulator',
  'deploy', 'refactor', 'git diff', 'log', 'server', 'backend', 'frontend', 'bug', 'fix', 'plan', 'code', 'review',
  'docker', 'async', 'await', 'rebase', 'staging', 'release', 'API', 'JSON', 'widget', 'provider',
];

/// Whisper keeps roughly the last 224 tokens of the prompt; this many characters stays well inside that.
const int maxPromptChars = 480;

final RegExp _fileName = RegExp(r'\b[\w\-]+\.(?:dart|ts|tsx|js|jsx|swift|kt|java|py|rb|go|rs|json|yaml|yml|md|gradle|plist|sh|html|css|sql|toml)\b');
final RegExp _identifier = RegExp(r'\b(?:[a-z]+(?:[A-Z][a-z0-9]+)+|[A-Z][a-z0-9]+(?:[A-Z][a-z0-9]+)+|[a-z0-9]+(?:_[a-z0-9]+)+)\b');

/// File names and identifiers (camelCase, PascalCase, snake_case) from recent chat text, newest first,
/// without duplicates. These are the words a speaker is most likely to say next.
List<String> technicalWords(Iterable<String> recentTexts, {int limit = 12}) {
  final List<String> found = <String>[];
  final Set<String> seen = <String>{};
  for (final String text in recentTexts.toList().reversed) {
    final List<String> hits = <String>[
      ..._fileName.allMatches(text).map((RegExpMatch m) => m.group(0)!),
      ..._identifier.allMatches(text).map((RegExpMatch m) => m.group(0)!),
    ];
    for (final String hit in hits.reversed) {
      if (hit.length < 4 || hit.length > 40 || !seen.add(hit.toLowerCase())) continue;
      found.add(hit);
      if (found.length >= limit) return found;
    }
  }
  return found;
}

/// The initial prompt for one dictation: session words first (they are the rarest), then the fixed glossary,
/// cut at [maxChars] on a word boundary.
String buildVoicePrompt({String? project, String? branch, Iterable<String> recentTexts = const <String>[], int maxChars = maxPromptChars}) {
  final List<String> terms = <String>[
    if (project != null && project.trim().isNotEmpty) project.trim(),
    if (branch != null && branch.trim().isNotEmpty) branch.trim(),
    ...technicalWords(recentTexts.toList().reversed.take(20).toList().reversed),
    ...devGlossary,
  ];
  final Set<String> seen = <String>{};
  final StringBuffer out = StringBuffer('Lập trình viên nói tiếng Việt, xen thuật ngữ tiếng Anh: ');
  bool first = true;
  for (final String term in terms) {
    if (!seen.add(term.toLowerCase())) continue;
    final String piece = first ? term : ', $term';
    if (out.length + piece.length + 1 > maxChars) break;
    out.write(piece);
    first = false;
  }
  out.write('.');
  return out.toString();
}
