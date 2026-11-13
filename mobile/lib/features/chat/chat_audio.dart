import 'dart:async';
import 'dart:typed_data';

import 'package:audioplayers/audioplayers.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../app/theme/tokens.dart';
import '../../core/net/bridge_client.dart';
import '../../core/protocol/models.dart';
import '../../l10n/app_localizations.dart';
import '../connection/connection.dart';
import 'chat_cache.dart';

/// Brings an audio file to the phone once: the disk copy if there is one, else the Mac sends it in pieces. Two chips
/// asking for the same file share one transfer.
class ChatAudioLoader {
  ChatAudioLoader(this._cache, this._client);
  final ChatCache _cache;
  final BridgeClient? Function() _client;

  final Map<String, Future<String>> _inFlight = <String, Future<String>>{};

  /// The local path of [audio]. [onProgress] gets 0..1 while the Mac is sending it.
  Future<String> load(String sessionId, ChatAudio audio, {void Function(double fraction)? onProgress}) {
    final String key = '$sessionId|${audio.id}';
    return _inFlight[key] ??= _fetch(sessionId, audio, onProgress).whenComplete(() {
      // A block body: returning the removed future would make this wait on itself.
      _inFlight.remove(key);
    });
  }

  Future<String> _fetch(String sessionId, ChatAudio audio, void Function(double fraction)? onProgress) async {
    final String? local = await _cache.loadAudio(sessionId, audio);
    if (local != null) return local;
    final BridgeClient? client = _client();
    if (client == null || !client.isConnected) throw const BridgeException('disconnected', 'Chưa kết nối tới Mac.');
    final Uint8List bytes = await client.chatAudio(sessionId, audio.id, onProgress: onProgress);
    try {
      return await _cache.saveAudio(sessionId, audio, bytes);
    } on Object catch (error) {
      debugPrint('ChatAudioLoader.save($sessionId, ${audio.id}): $error');
      throw const BridgeException('storage', 'Không lưu được tệp âm thanh.');
    }
  }
}

final Provider<ChatAudioLoader> chatAudioLoaderProvider = Provider<ChatAudioLoader>((Ref ref) {
  final ChatCache cache = ref.watch(chatCacheProvider);
  return ChatAudioLoader(cache, () => ref.read(bridgeClientProvider));
});

/// The one player of the app: starting a file stops the one playing, so voices never overlap. The engine is created
/// on first use, so nothing touches the platform until someone presses play.
class ChatAudioPlayback extends ChangeNotifier {
  ChatAudioPlayback({AudioPlayer Function()? createPlayer}) : _createPlayer = createPlayer ?? AudioPlayer.new;
  final AudioPlayer Function() _createPlayer;

  AudioPlayer? _player;
  final List<StreamSubscription<Object?>> _subscriptions = <StreamSubscription<Object?>>[];

  /// The chip whose file is loaded (playing or paused), or null.
  String? get activeKey => _activeKey;
  String? _activeKey;
  bool get isPlaying => _playing;
  bool _playing = false;
  Duration get position => _position;
  Duration _position = Duration.zero;
  Duration get duration => _duration;
  Duration _duration = Duration.zero;

  AudioPlayer _engine() {
    final AudioPlayer? existing = _player;
    if (existing != null) return existing;
    final AudioPlayer player = _createPlayer();
    _subscriptions
      ..add(player.onPositionChanged.listen((Duration value) {
        _position = value;
        notifyListeners();
      }))
      ..add(player.onDurationChanged.listen((Duration value) {
        _duration = value;
        notifyListeners();
      }))
      ..add(player.onPlayerComplete.listen((void _) {
        _playing = false;
        _activeKey = null;
        _position = Duration.zero;
        notifyListeners();
      }));
    return _player = player;
  }

  /// Plays [path] for [key], or pauses and resumes it when it is already the loaded file.
  Future<void> toggle(String key, String path) async {
    final AudioPlayer player = _engine();
    if (_activeKey == key) {
      if (_playing) {
        await player.pause();
        _playing = false;
      } else {
        await player.resume();
        _playing = true;
      }
      notifyListeners();
      return;
    }
    await player.stop();
    _activeKey = key;
    _position = Duration.zero;
    _duration = Duration.zero;
    _playing = true;
    notifyListeners();
    try {
      await player.play(DeviceFileSource(path));
    } on Object catch (error) {
      debugPrint('ChatAudioPlayback.play($path): $error');
      _activeKey = null;
      _playing = false;
      notifyListeners();
      rethrow;
    }
  }

  /// Marks [key] as the playing chip without an engine, for tests that stand in for the platform player.
  @visibleForTesting
  void debugActivate(String key) {
    _activeKey = key;
    _playing = true;
    notifyListeners();
  }

  @override
  void dispose() {
    for (final StreamSubscription<Object?> subscription in _subscriptions) {
      unawaited(subscription.cancel());
    }
    unawaited(_player?.dispose());
    super.dispose();
  }
}

final Provider<ChatAudioPlayback> chatAudioPlaybackProvider = Provider<ChatAudioPlayback>((Ref ref) {
  final ChatAudioPlayback playback = ChatAudioPlayback();
  ref.onDispose(playback.dispose);
  return playback;
});

/// The audio files of one message, one chip each.
class MessageAudio extends StatelessWidget {
  const MessageAudio({super.key, required this.sessionId, required this.audio});
  final String sessionId;
  final List<ChatAudio> audio;

  @override
  Widget build(BuildContext context) {
    if (audio.isEmpty) return const SizedBox.shrink();
    return Padding(
      padding: const EdgeInsets.only(top: Space.sm),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: <Widget>[
          for (final ChatAudio file in audio)
            Padding(
              padding: const EdgeInsets.only(bottom: Space.xs),
              child: AudioChip(key: ValueKey<String>(file.id), sessionId: sessionId, audio: file),
            ),
        ],
      ),
    );
  }
}

enum AudioLoad { idle, loading, ready, failed }

/// Play or pause, the file name and a thin progress bar. The file comes from the Mac when first played, then from
/// the phone's disk.
class AudioChip extends ConsumerStatefulWidget {
  const AudioChip({super.key, required this.sessionId, required this.audio});
  final String sessionId;
  final ChatAudio audio;

  @override
  ConsumerState<AudioChip> createState() => _AudioChipState();
}

class _AudioChipState extends ConsumerState<AudioChip> {
  AudioLoad _phase = AudioLoad.idle;
  double _fraction = 0;
  String? _path;

  String get _key => '${widget.sessionId}|${widget.audio.id}';

  Future<void> _press() async {
    final ChatAudioPlayback playback = ref.read(chatAudioPlaybackProvider);
    if (_phase == AudioLoad.loading) return;
    try {
      if (_path == null) {
        setState(() {
          _phase = AudioLoad.loading;
          _fraction = 0;
        });
        final String path = await ref.read(chatAudioLoaderProvider).load(
              widget.sessionId,
              widget.audio,
              onProgress: (double fraction) {
                if (mounted) setState(() => _fraction = fraction);
              },
            );
        if (!mounted) return;
        _path = path;
      }
      await playback.toggle(_key, _path!);
      if (mounted) setState(() => _phase = AudioLoad.ready);
    } on BridgeException catch (error) {
      debugPrint('AudioChip(${widget.audio.id}): ${error.code} ${error.message}');
      if (mounted) setState(() => _phase = AudioLoad.failed);
    } on Object catch (error) {
      // The player rejected the file: the next press fetches it again.
      debugPrint('AudioChip(${widget.audio.id}): $error');
      if (mounted) {
        setState(() {
          _path = null;
          _phase = AudioLoad.failed;
        });
      }
    }
  }

  static String _clock(Duration value) {
    final int seconds = value.inSeconds;
    return '${seconds ~/ 60}:${(seconds % 60).toString().padLeft(2, '0')}';
  }

  @override
  Widget build(BuildContext context) {
    final ChatAudioPlayback playback = ref.watch(chatAudioPlaybackProvider);
    return ListenableBuilder(listenable: playback, builder: (BuildContext context, Widget? _) => _chip(context, playback));
  }

  Widget _chip(BuildContext context, ChatAudioPlayback playback) {
    final Kit kit = Kit.of(context);
    final AppL10n l10n = AppL10n.of(context);
    final bool active = playback.activeKey == _key;
    final bool playing = active && playback.isPlaying;
    final bool failed = _phase == AudioLoad.failed;
    final bool loading = _phase == AudioLoad.loading;
    final double progress = active && playback.duration.inMilliseconds > 0
        ? (playback.position.inMilliseconds / playback.duration.inMilliseconds).clamp(0.0, 1.0)
        : 0;

    final Widget icon = loading
        ? SizedBox(width: 20, height: 20, child: CircularProgressIndicator(strokeWidth: 2, value: _fraction > 0 ? _fraction : null))
        : Icon(failed ? Icons.refresh : playing ? Icons.pause : Icons.play_arrow, size: 22, color: failed ? kit.danger : kit.accentText);

    return Semantics(
      button: true,
      label: failed ? l10n.audioFailed : playing ? l10n.audioPause : l10n.audioPlay,
      child: InkWell(
        borderRadius: BorderRadius.circular(Radii.sm),
        onTap: loading ? null : () => unawaited(_press()),
        child: Container(
          padding: const EdgeInsets.symmetric(horizontal: Space.md, vertical: Space.sm),
          decoration: BoxDecoration(color: kit.sidebar, borderRadius: BorderRadius.circular(Radii.sm), border: Border.all(color: kit.hairline)),
          child: Row(
            children: <Widget>[
              SizedBox(width: 28, height: 28, child: Center(child: icon)),
              const SizedBox(width: Space.sm),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  mainAxisSize: MainAxisSize.min,
                  children: <Widget>[
                    Text(widget.audio.name, maxLines: 1, overflow: TextOverflow.ellipsis, style: kit.label.copyWith(color: kit.text)),
                    const SizedBox(height: Space.xs),
                    if (failed)
                      Text(l10n.audioFailed, style: kit.labelSm.copyWith(color: kit.danger))
                    else
                      ClipRRect(
                        borderRadius: BorderRadius.circular(2),
                        child: LinearProgressIndicator(
                          minHeight: 3,
                          value: loading ? null : progress,
                          backgroundColor: kit.hairline,
                          color: kit.accent,
                        ),
                      ),
                  ],
                ),
              ),
              if (active && !failed) ...<Widget>[
                const SizedBox(width: Space.sm),
                Text('${_clock(playback.position)} / ${_clock(playback.duration)}', style: kit.labelSm),
              ],
            ],
          ),
        ),
      ),
    );
  }
}
