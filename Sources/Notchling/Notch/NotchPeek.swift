import SwiftUI

struct PeekView: View {
    @ObservedObject var model: NotchModel
    let agent: Agent

    var body: some View {
        Group {
            if let notice = model.notice, notice.agentID == agent.id {
                HStack(alignment: .top, spacing: 8) {
                    Image(systemName: "gauge.with.dots.needle.67percent").foregroundColor(.orange)
                    Text(notice.text)
                        .font(.system(size: 12.5)).foregroundColor(.white.opacity(0.9))
                        .lineLimit(3)
                    Spacer(minLength: 0)
                }
                .padding(.horizontal, 12)
                .padding(.top, 8)
                .contentShape(Rectangle())
                .onTapGesture { model.go(.collapsed) }
            } else if let request = model.permission(agent.id) {
                PermissionCard(model: model, agent: agent, request: request, showsAgent: true).part("Notch Permission Card", .topTrailing)
                    .reportsCardHeight(to: model)
                    .padding(.horizontal, 12)
                    .padding(.top, 6)
            } else if let q = model.question(agent.id) {
                QuestionCard(model: model, agent: agent, question: q, showsAgent: true, bodyLimit: QuestionCard.peekBodyLimit).part("Notch Question Card", .topTrailing)
                    .reportsCardHeight(to: model)
                    .padding(.horizontal, 12)
                    .padding(.top, 6)
            } else if model.canReplyInPeek(agent.id) {
                ReplyPeek(model: model, agent: agent).part("Reply Peek", .topTrailing)
                    .reportsCardHeight(to: model)
                    .padding(.horizontal, 12)
                    .padding(.top, 6)
            } else {
                VStack(alignment: .leading, spacing: 6) {
                    HStack(spacing: 8) {
                        Circle().fill(agent.color).frame(width: 8, height: 8)
                        Text(agent.name).font(.system(size: 13, weight: .semibold)).lineLimit(1)
                        Spacer()
                        Tag(text: agent.host)
                        Text(agent.group == .needsInput ? "Reply ›" : "Open ›")
                            .font(.system(size: 11)).foregroundColor(.secondary)
                    }
                    if let text = model.peekText(agent).map(Self.lastParagraph) {
                        Text(Bubble.markdown(text))
                            .font(.system(size: 12.5))
                            .foregroundColor(.white.opacity(0.85))
                            .lineLimit(3)
                            .padding(.leading, 16)
                    }
                }
                .padding(.horizontal, 12)
                .padding(.top, 6)
                .contentShape(Rectangle())
                .onTapGesture { model.open(agent.id) }
                .pointerCursor()
            }
        }
        .onExitCommand { model.go(.collapsed) }
    }

    /// The last paragraph is usually the question or the summary.
    static func lastParagraph(_ text: String) -> String {
        let paragraph = text.components(separatedBy: "\n\n")
            .last { !$0.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty } ?? text
        return String(paragraph.prefix(400))
    }
}

/// A peek for an agent waiting on a plain reply: its whole last message (scrolling when long),
/// the reply it suggests, and a box to answer in.
struct ReplyPeek: View {
    @ObservedObject var model: NotchModel
    let agent: Agent

    var body: some View {
        let text = model.peekText(agent) ?? ""
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 8) {
                Circle().fill(agent.color).frame(width: 8, height: 8)
                Text(agent.name).font(.system(size: 13, weight: .semibold)).lineLimit(1)
                Spacer()
                Tag(text: agent.host)
                Button("Open ›") { model.open(agent.id) }
                    .buttonStyle(.kitPlain).font(.system(size: 11)).foregroundColor(.secondary)
                    .pointerCursor()
                HideButton(model: model).part("Hide Button", .bottom)
            }
            .frame(height: 22)
            if !text.isEmpty {
                ScrollingBody(limit: 260, resetKey: 0) {
                    Text(Bubble.markdown(text))
                        .font(.system(size: 13))
                        .foregroundColor(.white.opacity(0.9))
                        .textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
            }
            if let suggested = model.suggestion(for: agent) {
                // Spelled out in full: as a placeholder it was cut off and read like a typed answer.
                Button { model.draft = suggested } label: {
                    HStack(alignment: .firstTextBaseline, spacing: 8) {
                        Text("Suggested reply").font(.system(size: 11, weight: .semibold)).foregroundColor(.orange)
                        Text(suggested).font(.system(size: 12.5)).foregroundColor(.white.opacity(0.85))
                            .fixedSize(horizontal: false, vertical: true)
                        Spacer(minLength: 0)
                    }
                    .padding(.horizontal, 10).padding(.vertical, 7)
                    .background(RoundedRectangle(cornerRadius: 8).fill(Color.orange.opacity(0.15)))
                    .contentShape(Rectangle())
                }
                .buttonStyle(.kitPlain)
                .pointerCursor()
                .help("Click, or press Tab, to put it in the box")
            }
            PeekReplyBox(model: model, agent: agent).part("Peek Reply Box", .trailing)
        }
        .padding(.bottom, 4)
    }
}

/// Folds a peek away; what it showed stays in the list.
struct HideButton: View {
    @ObservedObject var model: NotchModel

    var body: some View {
        Button { model.go(.collapsed) } label: {
            Image(systemName: "xmark").font(.system(size: 11, weight: .semibold))
        }
        .buttonStyle(.kitPlain).foregroundColor(.secondary)
        .pointerCursor()
        .help("Hide; it stays in the list (Esc)")
    }
}

/// The one-line reply box under a `ReplyPeek`; Tab takes the suggested reply.
struct PeekReplyBox: View {
    @ObservedObject var model: NotchModel
    let agent: Agent
    @FocusState private var focused: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack(spacing: 8) {
                TextField("Reply…", text: $model.draft)
                    .textFieldStyle(.plain)
                    .font(.system(size: 13))
                    .focused($focused)
                    .onSubmit { model.send() }
                    .padding(.horizontal, 10)
                    .frame(height: 32)
                    .background(RoundedRectangle(cornerRadius: 10).fill(Color.white.opacity(0.09)))
                    .disabled(model.sending)
                Button { model.send() } label: {
                    if model.sending {
                        ProgressView().controlSize(.small).frame(width: 24, height: 24)
                    } else {
                        Image(systemName: "arrow.up.circle.fill").font(.system(size: 24))
                    }
                }
                .buttonStyle(.kitPlain)
                .pointerCursor()
                .foregroundColor(model.draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? .secondary : .orange)
                .disabled(model.sending)
            }
            if let error = model.sendError {
                Text(error).font(.system(size: 11)).foregroundColor(.red).lineLimit(1)
            }
        }
        // Takes keys only once the pointer is over the peek, which makes the panel key.
        .onAppear { focused = true }
    }
}
