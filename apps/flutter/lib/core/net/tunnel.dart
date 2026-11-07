import '../protocol/json.dart';

/// The Mac's route through the Cloudflare tunnel, from the pairing QR (`tunnel` block): the wss URL and the
/// Cloudflare Access service token that lets this phone through Cloudflare. The token is only meant for
/// this URL's host (see [headersFor]).
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

  /// Throws [FormatException] unless this is a wss URL with a host and both token parts are set.
  void validate() {
    final Uri? parsed = Uri.tryParse(url);
    if (parsed == null || parsed.scheme != 'wss' || parsed.host.isEmpty) throw const FormatException('tunnel url must be wss://host');
    if (accessClientId.isEmpty || accessClientSecret.isEmpty) throw const FormatException('tunnel token is incomplete');
  }

  /// The Access headers, but only for [target] on the tunnel's own host. Anything else (a LAN address, a
  /// redirect to another site) gets none, so the service token cannot leak off the tunnel.
  Map<String, String> headersFor(Uri target) {
    if (target.host.toLowerCase() != host.toLowerCase()) return const <String, String>{};
    return <String, String>{'CF-Access-Client-Id': accessClientId, 'CF-Access-Client-Secret': accessClientSecret};
  }

  Json toJson() => <String, Object?>{'url': url, 'accessClientId': accessClientId, 'accessClientSecret': accessClientSecret};
}
