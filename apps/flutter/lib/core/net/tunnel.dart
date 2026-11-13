import '../protocol/json.dart';

class TunnelInfo {
  const TunnelInfo({required this.url, required this.accessClientId, required this.accessClientSecret});

  factory TunnelInfo.fromJson(Json json) => TunnelInfo(
        url: json.str('url'),
        accessClientId: json.str('accessClientId'),
        accessClientSecret: json.str('accessClientSecret'),
      );

  final String url;
  final String accessClientId;
  final String accessClientSecret;

  Uri get uri => Uri.parse(url);
  String get host => uri.host;

  void validate() {
    final Uri? parsed = Uri.tryParse(url);
    if (parsed == null || parsed.scheme != 'wss' || parsed.host.isEmpty) throw const FormatException('tunnel url must be wss://host');
    if (accessClientId.isEmpty || accessClientSecret.isEmpty) throw const FormatException('tunnel token is incomplete');
  }

  Map<String, String> headersFor(Uri target) {
    if (target.host.toLowerCase() != host.toLowerCase()) return const <String, String>{};
    return <String, String>{'CF-Access-Client-Id': accessClientId, 'CF-Access-Client-Secret': accessClientSecret};
  }

  Json toJson() => <String, Object?>{'url': url, 'accessClientId': accessClientId, 'accessClientSecret': accessClientSecret};
}
