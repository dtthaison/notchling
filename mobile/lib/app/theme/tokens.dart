import 'package:flutter/material.dart';

/// Night Desk tokens from Console/DESIGN.md. Names follow the DESIGN.md token names.
/// Nothing outside this folder writes a hex color, font name, radius or spacing value.
abstract final class Token {
  static const Color neutral = Color(0xFF101319);
  static const Color surface = Color(0xFF171B23);
  static const Color surfaceRaised = Color(0xFF1E2330);
  static const Color border = Color(0xFF262C38);
  static const Color onSurface = Color(0xFFE5E9F0);
  static const Color muted = Color(0xFF9BA5B5);
  static const Color primary = Color(0xFF38BDF8);
  static const Color onPrimary = Color(0xFF04202E);
  static const Color statusInput = Color(0xFFF5B544);
  static const Color onStatusInput = Color(0xFF2A1B00);
  static const Color statusRunning = Color(0xFF34D399);
  static const Color statusError = Color(0xFFF87171);

  /// Only ever the 8 px dot: 3.7:1 on `surface`, so no text uses it.
  static const Color statusExited = Color(0xFF6B7488);
}

abstract final class Radii {
  static const double sm = 4;
  static const double md = 6;
  static const double lg = 8;
}

abstract final class Space {
  static const double xs = 4;
  static const double sm = 8;
  static const double md = 12;
  static const double lg = 16;
  static const double xl = 24;
}

abstract final class Metric {
  /// Rows are 32 px on the Mac; a phone needs a 48 dp target.
  static const double rowHeight = 48;
  static const double subagentRowHeight = 44;
  static const double minTarget = 48;
  static const double inputHeight = 48;
  static const double buttonHeight = 48;

  /// From this width the three panels sit side by side like the Mac.
  static const double threePanelBreakpoint = 840;
  static const double maxMessageWidth = 72 * 7;
}

abstract final class Fonts {
  static const String sans = 'Geist';
  static const String mono = 'GeistMono';
}

/// DESIGN.md typography. Sizes scale with the system font size (large text safe).
abstract final class TypeToken {
  static const TextStyle panelTitle = TextStyle(
      fontFamily: Fonts.sans, fontSize: 11, fontWeight: FontWeight.w600, letterSpacing: 0.66, height: 1.3, color: Token.muted);
  static const TextStyle titleMd =
      TextStyle(fontFamily: Fonts.sans, fontSize: 15, fontWeight: FontWeight.w600, height: 1.35, color: Token.onSurface);
  static const TextStyle bodyMd =
      TextStyle(fontFamily: Fonts.sans, fontSize: 13, fontWeight: FontWeight.w400, height: 1.5, color: Token.onSurface);
  static const TextStyle bodyMdStrong =
      TextStyle(fontFamily: Fonts.sans, fontSize: 13, fontWeight: FontWeight.w600, height: 1.5, color: Token.onSurface);
  static const TextStyle bodySm =
      TextStyle(fontFamily: Fonts.sans, fontSize: 12, fontWeight: FontWeight.w400, height: 1.45, color: Token.muted);
  static const TextStyle labelSm = TextStyle(
      fontFamily: Fonts.sans,
      fontSize: 11,
      fontWeight: FontWeight.w500,
      height: 1.3,
      color: Token.muted,
      fontFeatures: <FontFeature>[FontFeature.tabularFigures()]);
  static const TextStyle codeSm =
      TextStyle(fontFamily: Fonts.mono, fontSize: 12, fontWeight: FontWeight.w400, height: 1.5, color: Token.muted);
}

ThemeData buildTheme() {
  const ColorScheme scheme = ColorScheme.dark(
    surface: Token.surface,
    onSurface: Token.onSurface,
    primary: Token.primary,
    onPrimary: Token.onPrimary,
    secondary: Token.muted,
    error: Token.statusError,
    outline: Token.border,
    surfaceContainerHighest: Token.surfaceRaised,
  );
  return ThemeData(
    useMaterial3: true,
    brightness: Brightness.dark,
    colorScheme: scheme,
    scaffoldBackgroundColor: Token.neutral,
    canvasColor: Token.surface,
    fontFamily: Fonts.sans,
    textTheme: const TextTheme(
      bodyLarge: TypeToken.bodyMd,
      bodyMedium: TypeToken.bodyMd,
      bodySmall: TypeToken.bodySm,
      titleMedium: TypeToken.titleMd,
      labelSmall: TypeToken.labelSm,
    ),
    dividerTheme: const DividerThemeData(color: Token.border, thickness: 1, space: 1),
    splashFactory: NoSplash.splashFactory,
    navigationBarTheme: NavigationBarThemeData(
      backgroundColor: Token.surface,
      indicatorColor: Token.surfaceRaised,
      labelTextStyle: WidgetStateProperty.resolveWith(
          (Set<WidgetState> states) => TypeToken.labelSm.copyWith(color: states.contains(WidgetState.selected) ? Token.primary : Token.muted)),
      iconTheme: WidgetStateProperty.resolveWith(
          (Set<WidgetState> states) => IconThemeData(color: states.contains(WidgetState.selected) ? Token.primary : Token.muted)),
    ),
    inputDecorationTheme: InputDecorationTheme(
      filled: true,
      fillColor: Token.neutral,
      hintStyle: TypeToken.bodyMd.copyWith(color: Token.muted),
      contentPadding: const EdgeInsets.symmetric(horizontal: Space.md, vertical: Space.md),
      border: OutlineInputBorder(borderRadius: BorderRadius.circular(Radii.md), borderSide: const BorderSide(color: Token.border)),
      enabledBorder: OutlineInputBorder(borderRadius: BorderRadius.circular(Radii.md), borderSide: const BorderSide(color: Token.border)),
      focusedBorder: OutlineInputBorder(borderRadius: BorderRadius.circular(Radii.md), borderSide: const BorderSide(color: Token.primary, width: 2)),
    ),
    snackBarTheme: SnackBarThemeData(
      backgroundColor: Token.surfaceRaised,
      contentTextStyle: TypeToken.bodyMd,
      behavior: SnackBarBehavior.floating,
      shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(Radii.md), side: const BorderSide(color: Token.border)),
    ),
    dialogTheme: DialogThemeData(
      backgroundColor: Token.surfaceRaised,
      shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(Radii.lg), side: const BorderSide(color: Token.border)),
      titleTextStyle: TypeToken.titleMd,
      contentTextStyle: TypeToken.bodyMd,
    ),
    bottomSheetTheme: const BottomSheetThemeData(backgroundColor: Token.surface, modalBackgroundColor: Token.surface),
  );
}
