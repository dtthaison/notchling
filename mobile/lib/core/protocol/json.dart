/// Typed access to decoded JSON. A wrong shape throws [FormatException] instead of a cast error.
typedef Json = Map<String, Object?>;

extension JsonAccess on Json {
  String str(String key) {
    final Object? value = this[key];
    if (value is String) return value;
    throw FormatException('"$key" is not a string', value);
  }

  String? strOrNull(String key) {
    final Object? value = this[key];
    return value is String ? value : null;
  }

  int integer(String key, {int? orElse}) {
    final Object? value = this[key];
    if (value is int) return value;
    if (value is num) return value.toInt();
    if (orElse != null) return orElse;
    throw FormatException('"$key" is not a number', value);
  }

  double number(String key, {double orElse = 0}) {
    final Object? value = this[key];
    return value is num ? value.toDouble() : orElse;
  }

  bool boolean(String key, {bool orElse = false}) {
    final Object? value = this[key];
    return value is bool ? value : orElse;
  }

  Json object(String key) {
    final Object? value = this[key];
    if (value is Map<String, Object?>) return value;
    if (value is Map<Object?, Object?>) return value.cast<String, Object?>();
    throw FormatException('"$key" is not an object', value);
  }

  List<Json> objects(String key) {
    final Object? value = this[key];
    if (value == null) return <Json>[];
    if (value is! List<Object?>) throw FormatException('"$key" is not a list', value);
    return value.map((Object? item) {
      if (item is Map<String, Object?>) return item;
      if (item is Map<Object?, Object?>) return item.cast<String, Object?>();
      throw FormatException('"$key" holds a non-object', item);
    }).toList();
  }

  List<String> strings(String key) {
    final Object? value = this[key];
    if (value == null) return <String>[];
    if (value is! List<Object?>) throw FormatException('"$key" is not a list', value);
    return value.whereType<String>().toList();
  }
}
