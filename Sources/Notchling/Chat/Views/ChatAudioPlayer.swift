import AVFoundation
import ConsoleCore
import SwiftUI

/// The one audio player of the app: starting a file stops the one playing, so voices never overlap.
@MainActor
final class AudioPlayback: NSObject, ObservableObject, AVAudioPlayerDelegate {
    static let shared = AudioPlayback()

    /// The file that is loaded (playing or paused).
    @Published private(set) var path: String?
    @Published private(set) var isPlaying = false
    @Published private(set) var elapsed: TimeInterval = 0
    @Published private(set) var failed: String?
    private var player: AVAudioPlayer?
    private var timer: Timer?

    func toggle(_ file: String) {
        if path == file, let player {
            if player.isPlaying { player.pause(); isPlaying = false; stopTimer() } else { resume(player) }
            return
        }
        stop()
        do {
            let next = try AVAudioPlayer(contentsOf: URL(fileURLWithPath: file))
            next.delegate = self
            guard next.prepareToPlay() else { throw CocoaError(.fileReadCorruptFile) }
            player = next
            path = file
            failed = nil
            resume(next)
        } catch {
            NSLog("AudioPlayback.toggle %@: %@", file, "\(error)")
            failed = file
        }
    }

    func stop() {
        player?.stop()
        player = nil
        path = nil
        isPlaying = false
        elapsed = 0
        stopTimer()
    }

    private func resume(_ player: AVAudioPlayer) {
        player.play()
        isPlaying = true
        stopTimer()
        timer = Timer.scheduledTimer(withTimeInterval: 0.25, repeats: true) { [weak self] _ in
            Task { @MainActor in self?.elapsed = self?.player?.currentTime ?? 0 }
        }
    }

    private func stopTimer() { timer?.invalidate(); timer = nil }

    nonisolated func audioPlayerDidFinishPlaying(_ player: AVAudioPlayer, successfully flag: Bool) {
        Task { @MainActor in self.stop() }
    }
}

/// One compact row per audio file an assistant message names: play or pause, the file name, elapsed and length.
struct AudioPlayerRows: View {
    let text: String
    @Environment(\.chatFolder) private var folder
    @State private var files: [String] = []

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            ForEach(files, id: \.self) { AudioPlayerRow(path: $0) }
        }
        .padding(.top, files.isEmpty ? 0 : 6)
        .task(id: text + (folder ?? "")) {
            let message = text, cwd = folder
            files = await Task.detached(priority: .utility) { ChatAudio.files(in: message, cwd: cwd) }.value
        }
    }
}

struct AudioPlayerRow: View {
    let path: String
    @ObservedObject private var playback = AudioPlayback.shared
    @State private var duration: TimeInterval?

    private var current: Bool { playback.path == path }

    var body: some View {
        HStack(spacing: 8) {
            Button { playback.toggle(path) } label: {
                KitIconView(icon: current && playback.isPlaying ? .pause : .play)
                    .frame(width: KitMetric.control, height: KitMetric.control)
            }
            .buttonStyle(.kit(.ghost))
            .help(current && playback.isPlaying ? "Pause" : "Play")
            Text((path as NSString).lastPathComponent)
                .kitType(.label).foregroundColor(KitInk.text).lineLimit(1).truncationMode(.middle)
            Spacer(minLength: 8)
            Text(timeLabel).kitType(.meta).foregroundColor(playback.failed == path ? KitInk.expired : KitInk.textMuted).monospacedDigit()
        }
        .padding(.leading, 4).padding(.trailing, 12)
        .kitCard(radius: KitMetric.radiusMd)
        .part("Audio Player")
        .task(id: path) { duration = await Self.length(of: path) }
    }

    private var timeLabel: String {
        if playback.failed == path { return "Can't play" }
        let total = duration.map(Self.clock) ?? "--:--"
        return current ? "\(Self.clock(playback.elapsed)) / \(total)" : total
    }

    static func clock(_ seconds: TimeInterval) -> String {
        let whole = max(0, Int(seconds.rounded()))
        return String(format: "%d:%02d", whole / 60, whole % 60)
    }

    private static func length(of path: String) async -> TimeInterval? {
        let asset = AVURLAsset(url: URL(fileURLWithPath: path))
        guard let time = try? await asset.load(.duration), time.isNumeric else { return nil }
        return time.seconds
    }
}
