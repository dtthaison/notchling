import 'package:flutter/material.dart';

import '../../app/theme/tokens.dart';

/// A small syntax highlighter: comments, strings, numbers, keywords and type names. One pass, no grammar per
/// language; the keyword set covers the languages Claude writes most (Dart, Swift, TS/JS, Python, Kotlin, Go,
/// Rust, shell). Good enough to read code on a phone; not an editor.
const Set<String> _keywords = <String>{
  'abstract',
  'as',
  'async',
  'await',
  'break',
  'case',
  'catch',
  'class',
  'const',
  'continue',
  'def',
  'default',
  'defer',
  'do',
  'else',
  'enum',
  'export',
  'extends',
  'extension',
  'false',
  'final',
  'finally',
  'fn',
  'for',
  'from',
  'func',
  'function',
  'guard',
  'if',
  'impl',
  'import',
  'in',
  'interface',
  'is',
  'let',
  'match',
  'mod',
  'mut',
  'new',
  'nil',
  'null',
  'of',
  'override',
  'private',
  'protocol',
  'pub',
  'public',
  'return',
  'self',
  'static',
  'struct',
  'super',
  'switch',
  'then',
  'this',
  'throw',
  'throws',
  'true',
  'try',
  'type',
  'typedef',
  'use',
  'val',
  'var',
  'void',
  'where',
  'while',
  'with',
  'yield',
  'fi',
  'done',
  'echo',
  'elif',
  'None',
  'True',
  'False',
  'lambda',
  'pass',
};

/// The language from a fence word or a file path ("swift", "main.dart"); null when unknown.
String? languageOf(String? hint) {
  if (hint == null || hint.isEmpty) return null;
  final String lower = hint.toLowerCase();
  final int dot = lower.lastIndexOf('.');
  return dot >= 0 ? lower.substring(dot + 1) : lower;
}

bool _hashComments(String? lang) => const <String>{
  'py',
  'python',
  'sh',
  'bash',
  'zsh',
  'shell',
  'yaml',
  'yml',
  'rb',
  'ruby',
  'toml',
  'conf',
}.contains(lang);

List<TextSpan> highlightCode(String code, Kit kit, {String? language}) {
  final bool hash = _hashComments(language);
  final bool plain =
      language == 'json' || language == 'text' || language == 'txt';
  final List<TextSpan> spans = <TextSpan>[];
  final StringBuffer plainRun = StringBuffer();
  void flush() {
    if (plainRun.isNotEmpty) {
      spans.add(TextSpan(text: plainRun.toString()));
      plainRun.clear();
    }
  }

  void emit(String text, Color color, {FontStyle? italic}) {
    flush();
    spans.add(
      TextSpan(
        text: text,
        style: TextStyle(color: color, fontStyle: italic),
      ),
    );
  }

  int i = 0;
  final int n = code.length;
  bool isIdStart(int c) =>
      (c >= 65 && c <= 90) || (c >= 97 && c <= 122) || c == 95;
  bool isDigit(int c) => c >= 48 && c <= 57;
  while (i < n) {
    final String ch = code[i];
    final int c = code.codeUnitAt(i);
    if (!plain &&
        ((ch == '/' && i + 1 < n && code[i + 1] == '/') ||
            (hash && ch == '#'))) {
      int end = code.indexOf('\n', i);
      if (end < 0) end = n;
      emit(code.substring(i, end), kit.textFaint, italic: FontStyle.italic);
      i = end;
    } else if (!plain && ch == '/' && i + 1 < n && code[i + 1] == '*') {
      int end = code.indexOf('*/', i + 2);
      end = end < 0 ? n : end + 2;
      emit(code.substring(i, end), kit.textFaint, italic: FontStyle.italic);
      i = end;
    } else if (ch == '"' || ch == "'" || ch == '`') {
      int j = i + 1;
      while (j < n && code[j] != ch && code[j] != '\n') {
        if (code[j] == r'\' && j + 1 < n) j++;
        j++;
      }
      j = j < n && code[j] == ch ? j + 1 : j;
      emit(code.substring(i, j), kit.syntaxString);
      i = j;
    } else if (isDigit(c) && (i == 0 || !isIdStart(code.codeUnitAt(i - 1)))) {
      int j = i + 1;
      while (j < n &&
          (isDigit(code.codeUnitAt(j)) ||
              code[j] == '.' ||
              code[j] == '_' ||
              code[j] == 'x')) {
        j++;
      }
      emit(code.substring(i, j), kit.syntaxNumber);
      i = j;
    } else if (isIdStart(c)) {
      int j = i + 1;
      while (j < n &&
          (isIdStart(code.codeUnitAt(j)) || isDigit(code.codeUnitAt(j)))) {
        j++;
      }
      final String word = code.substring(i, j);
      if (!plain && _keywords.contains(word)) {
        emit(word, kit.syntaxKeyword);
      } else if (!plain && c >= 65 && c <= 90 && word.length > 1) {
        emit(word, kit.syntaxType);
      } else {
        plainRun.write(word);
      }
      i = j;
    } else {
      plainRun.write(ch);
      i++;
    }
  }
  flush();
  return spans;
}
