import AppKit
import Combine
import SwiftUI
import UniformTypeIdentifiers

struct Notice: View {
    let text: String

    var body: some View {
        HStack(spacing: 10) {
            KitIconView(icon: .info, size: 13).foregroundColor(KitInk.textMuted)
            Text(text).kitType(.body).foregroundColor(KitInk.textMuted)
            Spacer()
        }
        .padding(.horizontal, 16).padding(.vertical, 12)
        .kitCard(radius: KitMetric.radiusComposer, fill: KitInk.card.opacity(0.6))
    }
}

/// A background job that runs a shell command: what it runs, how it ended, and the end of its output file.
/// Claude Code keeps no transcript for it; sessions it starts (`claude -p`) have their own.
struct CommandJobView: View {
    let agent: Agent
    let command: String
    @State private var output: String?

    private var outputPath: String? { CommandJob.outputPath(in: command, cwd: agent.cwd) }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 8) {
                KitIconView(icon: .terminal, size: 13).foregroundColor(KitInk.textMuted)
                Text("Command Job").kitType(.heading).foregroundColor(KitInk.text)
                Text(status).kitType(.meta).foregroundColor(agent.label == "Failed" ? KitInk.needs : KitInk.textMuted)
            }
            Text("It runs a shell command instead of a conversation, so there's no chat history. Sessions it starts with claude -p keep their own transcripts.")
                .kitType(.body).foregroundColor(KitInk.textMuted).fixedSize(horizontal: false, vertical: true)
            box(CommandJob.readable(command)).frame(maxHeight: 110)
            if let path = outputPath {
                HStack(spacing: 6) {
                    Text("Output").kitType(.section).foregroundColor(KitInk.textFaint)
                    Text((path as NSString).lastPathComponent).kitType(.meta).foregroundColor(KitInk.textFaint)
                        .lineLimit(1).truncationMode(.middle).help(path)
                }
                .padding(.top, 4)
                ScrollViewReader { proxy in
                    box(output ?? "The output file isn't there.", id: "output-end")
                        .onChange(of: output) { _ in proxy.scrollTo("output-end", anchor: .bottom) }
                        .onAppear { DispatchQueue.main.async { proxy.scrollTo("output-end", anchor: .bottom) } }
                }
                .part("Command Output", .top)
            }
            Spacer(minLength: 0)
        }
        .padding(24)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        // Read again every few seconds: a running command keeps writing.
        .task(id: outputPath) {
            while !Task.isCancelled {
                let path = outputPath
                let text = await Task.detached { path.flatMap { CommandJob.tail($0) } }.value
                if text != output { output = text }
                try? await Task.sleep(nanoseconds: 3_000_000_000)
            }
        }
    }

    private func box(_ text: String, id: String? = nil) -> some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 0) {
                Text(text).kitType(.mono).foregroundColor(KitInk.text)
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(12)
                Color.clear.frame(height: 1).id(id ?? "")
            }
        }
        .background(RoundedRectangle(cornerRadius: KitMetric.radiusMd).fill(KitInk.raised))
    }

    private var status: String {
        let state = [agent.label, agent.detail].compactMap { $0 }.filter { !$0.isEmpty }.joined(separator: " · ")
        return state + " · " + ChatFormat.age(agent.since)
    }
}

struct EmptyState: View {
    let pose: String
    let title: String
    let message: String
    var action: AnyView? = nil

    var body: some View {
        VStack(spacing: 10) {
            KitPet(pose: pose, size: 120)
            Text(title).kitType(.title).foregroundColor(KitInk.text)
            Text(message).kitType(.body).foregroundColor(KitInk.textMuted)
                .multilineTextAlignment(.center).frame(maxWidth: 320)
            if let action { action.padding(.top, 6) }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .padding(20)
    }
}

extension Notification.Name {
    /// A background session was removed (`claude rm`); the object is its agent id.
    static let sessionRemoved = Notification.Name("NotchlingSessionRemoved")
}
