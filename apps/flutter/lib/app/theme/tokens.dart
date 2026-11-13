import 'package:flutter/material.dart';

abstract final class _Palette {
  static const Color night0 = Color(0xFF0F0E0D);
  static const Color night1 = Color(0xFF151311);
  static const Color night2 = Color(0xFF1B1917);
  static const Color night3 = Color(0xFF242120);
  static const Color night4 = Color(0xFF2F2B28);
  static const Color paper0 = Color(0xFFFFFFFF);
  static const Color paper1 = Color(0xFFF4F0EA);
  static const Color paper2 = Color(0xFFFAF8F5);
  static const Color paper4 = Color(0xFFECE6DD);
  static const Color snow = Color(0xFFF3EEE8);
  static const Color fog = Color(0xFFA69D93);
  static const Color smoke = Color(0xFF736B62);
  static const Color soot = Color(0xFF1F1B17);
  static const Color stone = Color(0xFF6C6359);
  static const Color pebble = Color(0xFF9B9187);
  static const Color orange300 = Color(0xFFFFB47E);
  static const Color orange500 = Color(0xFFF7873B);
  static const Color orange700 = Color(0xFFB4540E);
  static const Color lime500 = Color(0xFF7DCB2C);
  static const Color lime700 = Color(0xFF3F7D0B);
  static const Color blue400 = Color(0xFF6EB0FF);
  static const Color blue600 = Color(0xFF2C78D4);
  static const Color pink400 = Color(0xFFFF7AB8);
  static const Color pink600 = Color(0xFFC2307A);
  static const Color red400 = Color(0xFFFF8A7A);
  static const Color red600 = Color(0xFFC2362B);
}

abstract final class Radii {
  static const double sm = 10;
  static const double md = 16;
  static const double card = 16;
  static const double bubble = 20;
  static const double bubbleTail = 6;
  static const double composer = 22;
  static const double sheet = 28;
  static const double full = 999;
}

abstract final class Space {
  static const double xs = 4;
  static const double sm = 8;
  static const double md = 12;
  static const double lg = 16;
  static const double xl = 24;
}

abstract final class Metric {
  static const double rowHeight = 56;
  static const double subagentRowHeight = 44;
  static const double minTarget = 48;
  static const double inputHeight = 48;
  static const double buttonHeight = 48;
  static const double threePanelBreakpoint = 840;
  static const double maxMessageWidth = 72 * 7;
}

abstract final class Fonts {
  static const String mono = 'GeistMono';
}

@immutable
class Kit extends ThemeExtension<Kit> {
  const Kit._({
    required this.brightness,
    required this.window,
    required this.sidebar,
    required this.card,
    required this.raised,
    required this.selection,
    required this.hairline,
    required this.text,
    required this.textMuted,
    required this.textFaint,
    required this.accent,
    required this.accentText,
    required this.onAccent,
    required this.accentWash,
    required this.needs,
    required this.working,
    required this.done,
    required this.shell,
    required this.shellWash,
    required this.onShell,
    required this.danger,
    required this.addWash,
    required this.removeWash,
    required this.add,
    required this.syntaxKeyword,
    required this.syntaxString,
    required this.syntaxNumber,
    required this.syntaxType,
    required this.glass,
    required this.highlight,
    required this.edge,
    required this.shadowTint,
  });

  static const Kit dark = Kit._(
    brightness: Brightness.dark,
    window: _Palette.night2,
    sidebar: _Palette.night1,
    card: _Palette.night3,
    raised: _Palette.night4,
    selection: Color(0x29F7873B),
    hairline: Color(0x14FFFFFF),
    text: _Palette.snow,
    textMuted: _Palette.fog,
    textFaint: _Palette.smoke,
    accent: _Palette.orange500,
    accentText: _Palette.orange300,
    onAccent: _Palette.night0,
    accentWash: Color(0x2EF7873B),
    needs: _Palette.orange500,
    working: _Palette.blue400,
    done: _Palette.lime500,
    shell: _Palette.pink400,
    shellWash: Color(0x24FF7AB8),
    onShell: _Palette.night0,
    danger: _Palette.red400,
    addWash: Color(0x297DCB2C),
    removeWash: Color(0x29FF8A7A),
    add: _Palette.lime500,
    syntaxKeyword: _Palette.orange300,
    syntaxString: Color(0xFFA5DB6B),
    syntaxNumber: _Palette.blue400,
    syntaxType: _Palette.pink400,
    glass: Color(0xB8151311),
    highlight: Color(0x0FFFFFFF),
    edge: Color(0x59000000),
    shadowTint: Color(0xFF000000),
  );

  static const Kit light = Kit._(
    brightness: Brightness.light,
    window: _Palette.paper2,
    sidebar: _Palette.paper1,
    card: _Palette.paper0,
    raised: _Palette.paper4,
    selection: Color(0x1FF7873B),
    hairline: Color(0x1F3B2A1A),
    text: _Palette.soot,
    textMuted: _Palette.stone,
    textFaint: _Palette.pebble,
    accent: _Palette.orange500,
    accentText: _Palette.orange700,
    onAccent: _Palette.soot,
    accentWash: Color(0x24F7873B),
    needs: _Palette.orange500,
    working: _Palette.blue600,
    done: _Palette.lime500,
    shell: _Palette.pink600,
    shellWash: Color(0x1AC2307A),
    onShell: _Palette.paper0,
    danger: _Palette.red600,
    addWash: Color(0x2E7DCB2C),
    removeWash: Color(0x24C2362B),
    add: _Palette.lime700,
    syntaxKeyword: _Palette.orange700,
    syntaxString: _Palette.lime700,
    syntaxNumber: _Palette.blue600,
    syntaxType: _Palette.pink600,
    glass: Color(0xC7F4F0EA),
    highlight: Color(0xB3FFFFFF),
    edge: Color(0x1F3B2A1A),
    shadowTint: Color(0xFF3B2A1A),
  );

  static Kit of(BuildContext context) =>
      Theme.of(context).extension<Kit>() ?? dark;

  final Brightness brightness;
  final Color window;
  final Color sidebar;
  final Color card;
  final Color raised;
  final Color selection;
  final Color hairline;
  final Color text;
  final Color textMuted;
  final Color textFaint;
  final Color accent;
  final Color accentText;

  final Color onAccent;
  final Color accentWash;
  final Color needs;
  final Color working;
  final Color done;

  final Color shell;
  final Color shellWash;
  final Color onShell;
  final Color danger;
  final Color addWash;
  final Color removeWash;
  final Color add;
  final Color syntaxKeyword;
  final Color syntaxString;
  final Color syntaxNumber;
  final Color syntaxType;

  final Color glass;
  final Color highlight;
  final Color edge;
  final Color shadowTint;

  bool get isDark => brightness == Brightness.dark;

  List<BoxShadow> get shadow1 => <BoxShadow>[
        BoxShadow(color: shadowTint.withValues(alpha: isDark ? 0.40 : 0.12), blurRadius: 3, offset: const Offset(0, 1)),
      ];

  List<BoxShadow> get shadow2 => <BoxShadow>[
        BoxShadow(color: shadowTint.withValues(alpha: isDark ? 0.40 : 0.12), blurRadius: 3, offset: const Offset(0, 1)),
        BoxShadow(color: shadowTint.withValues(alpha: isDark ? 0.35 : 0.14), blurRadius: 30, offset: const Offset(0, 10)),
      ];

  static const List<FontFeature> _tabular = <FontFeature>[
    FontFeature.tabularFigures(),
  ];
  TextStyle get title => TextStyle(
    fontSize: 19,
    fontWeight: FontWeight.w600,
    height: 1.3,
    color: text,
  );
  TextStyle get heading => TextStyle(
    fontSize: 15,
    fontWeight: FontWeight.w600,
    height: 1.4,
    color: text,
  );
  TextStyle get body => TextStyle(
    fontSize: 15,
    fontWeight: FontWeight.w400,
    height: 1.5,
    color: text,
  );
  TextStyle get label => TextStyle(
    fontSize: 13,
    fontWeight: FontWeight.w500,
    height: 1.35,
    color: text,
    fontFeatures: _tabular,
  );
  TextStyle get meta => TextStyle(
    fontSize: 12.5,
    fontWeight: FontWeight.w400,
    height: 1.4,
    color: textMuted,
    fontFeatures: _tabular,
  );
  TextStyle get section => TextStyle(
    fontSize: 12,
    fontWeight: FontWeight.w600,
    height: 1.3,
    letterSpacing: 0.7,
    color: textFaint,
  );
  TextStyle get mono => TextStyle(
    fontFamily: Fonts.mono,
    fontSize: 13,
    fontWeight: FontWeight.w400,
    height: 1.5,
    color: text,
  );

  TextStyle get panelTitle => section;
  TextStyle get titleMd => heading;
  TextStyle get bodyMd => body;
  TextStyle get bodyMdStrong => heading;
  TextStyle get bodySm => meta;
  TextStyle get labelSm => label.copyWith(color: textMuted);
  TextStyle get codeSm => mono.copyWith(fontSize: 12.5, color: textMuted);

  Color statusColor(Object group) => switch (group.toString()) {
    'needs' => needs,
    'working' => working,
    _ => done,
  };

  @override
  Kit copyWith() => this;

  @override
  Kit lerp(ThemeExtension<Kit>? other, double t) {
    if (other is! Kit) return this;
    Color c(Color a, Color b) => Color.lerp(a, b, t)!;
    final Kit o = other;
    return Kit._(
      brightness: t < 0.5 ? brightness : o.brightness,
      window: c(window, o.window),
      sidebar: c(sidebar, o.sidebar),
      card: c(card, o.card),
      raised: c(raised, o.raised),
      selection: c(selection, o.selection),
      hairline: c(hairline, o.hairline),
      text: c(text, o.text),
      textMuted: c(textMuted, o.textMuted),
      textFaint: c(textFaint, o.textFaint),
      accent: c(accent, o.accent),
      accentText: c(accentText, o.accentText),
      onAccent: c(onAccent, o.onAccent),
      accentWash: c(accentWash, o.accentWash),
      needs: c(needs, o.needs),
      working: c(working, o.working),
      done: c(done, o.done),
      shell: c(shell, o.shell),
      shellWash: c(shellWash, o.shellWash),
      onShell: c(onShell, o.onShell),
      danger: c(danger, o.danger),
      addWash: c(addWash, o.addWash),
      removeWash: c(removeWash, o.removeWash),
      add: c(add, o.add),
      syntaxKeyword: c(syntaxKeyword, o.syntaxKeyword),
      syntaxString: c(syntaxString, o.syntaxString),
      syntaxNumber: c(syntaxNumber, o.syntaxNumber),
      syntaxType: c(syntaxType, o.syntaxType),
      glass: c(glass, o.glass),
      highlight: c(highlight, o.highlight),
      edge: c(edge, o.edge),
      shadowTint: c(shadowTint, o.shadowTint),
    );
  }
}

ThemeData buildTheme([
  Brightness brightness = Brightness.dark,
  String? fontFamily,
]) {
  final Kit kit = brightness == Brightness.dark ? Kit.dark : Kit.light;
  final ColorScheme scheme = ColorScheme(
    brightness: brightness,
    primary: kit.accent,
    onPrimary: kit.onAccent,
    secondary: kit.textMuted,
    onSecondary: kit.window,
    error: kit.danger,
    onError: kit.window,
    surface: kit.window,
    onSurface: kit.text,
    outline: kit.hairline,
    surfaceContainerHighest: kit.raised,
    surfaceContainer: kit.card,
  );
  OutlineInputBorder border(Color color, [double width = 1]) =>
      OutlineInputBorder(
        borderRadius: BorderRadius.circular(Radii.sm),
        borderSide: BorderSide(color: color, width: width),
      );
  return ThemeData(
    useMaterial3: true,
    fontFamily: fontFamily,
    brightness: brightness,
    colorScheme: scheme,
    extensions: <ThemeExtension<Kit>>[kit],
    scaffoldBackgroundColor: kit.window,
    canvasColor: kit.sidebar,
    textTheme: TextTheme(
      bodyLarge: kit.body,
      bodyMedium: kit.body,
      bodySmall: kit.meta,
      titleMedium: kit.heading,
      labelSmall: kit.label,
      labelLarge: kit.label,
    ),
    dividerTheme: DividerThemeData(color: kit.hairline, thickness: 1, space: 1),
    splashFactory: NoSplash.splashFactory,
    pageTransitionsTheme: const PageTransitionsTheme(builders: <TargetPlatform, PageTransitionsBuilder>{
      TargetPlatform.android: FadeForwardsPageTransitionsBuilder(),
      TargetPlatform.iOS: FadeForwardsPageTransitionsBuilder(),
    }),
    highlightColor: kit.raised.withValues(alpha: 0.6),
    navigationBarTheme: NavigationBarThemeData(
      backgroundColor: kit.sidebar,
      indicatorColor: kit.selection,
      labelTextStyle: WidgetStateProperty.resolveWith(
        (Set<WidgetState> states) => kit.label.copyWith(
          color: states.contains(WidgetState.selected)
              ? kit.accentText
              : kit.textMuted,
        ),
      ),
      iconTheme: WidgetStateProperty.resolveWith(
        (Set<WidgetState> states) => IconThemeData(
          color: states.contains(WidgetState.selected)
              ? kit.accentText
              : kit.textMuted,
        ),
      ),
    ),
    inputDecorationTheme: InputDecorationTheme(
      filled: true,
      fillColor: kit.card,
      hintStyle: kit.body.copyWith(color: kit.textFaint),
      contentPadding: const EdgeInsets.symmetric(
        horizontal: Space.md,
        vertical: Space.md,
      ),
      border: border(kit.hairline),
      enabledBorder: border(kit.hairline),
      focusedBorder: border(kit.accent, 1.5),
    ),
    textSelectionTheme: TextSelectionThemeData(
      cursorColor: kit.accent,
      selectionColor: kit.accent.withValues(alpha: 0.3),
    ),
    snackBarTheme: SnackBarThemeData(
      backgroundColor: kit.raised,
      contentTextStyle: kit.body,
      behavior: SnackBarBehavior.floating,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(Radii.md),
        side: BorderSide(color: kit.hairline),
      ),
    ),
    dialogTheme: DialogThemeData(
      backgroundColor: kit.card,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(Radii.card),
        side: BorderSide(color: kit.hairline),
      ),
      titleTextStyle: kit.heading,
      contentTextStyle: kit.body,
    ),
    textButtonTheme: TextButtonThemeData(
      style: TextButton.styleFrom(foregroundColor: kit.accentText),
    ),
    bottomSheetTheme: BottomSheetThemeData(
      backgroundColor: kit.card,
      modalBackgroundColor: kit.card,
    ),
    progressIndicatorTheme: ProgressIndicatorThemeData(color: kit.accent),
  );
}
