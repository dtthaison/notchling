import AppKit
import Combine
import SwiftUI
import UniformTypeIdentifiers

/// The messages. Equatable so the notch's own updates (hover, peeks) don't re-lay out a long history.
struct ConversationView: View, Equatable {
    let conversation: Conversation
    let pending: ChatModel.Pending?
    /// Replies waiting for the one before to go out.
    var outbox: [String] = []
    /// The session is working: its newest tool step stays open and a live row shows what it's doing.
    let live: Bool
    let expansion: ChatExpansion
    var pageOpen = false
    /// Takes the newest queued or waiting message back to the reply box (not part of ==).
    var onEditLatest: (() -> Void)?
    /// Following the newest message: true until you scroll up, and again once you're back at the bottom.
    /// Only your own scrolling changes it (`UserScrollWatcher`), so content growing below can't be
    /// mistaken for a scroll and pull you back down while you read.
    @State private var atBottom = true
    /// While our own animated jump runs, don't also snap to the end.
    @State private var jumpingUntil = Date.distantPast
    /// Spaces out the follow-the-end corrections. A class, so setting it doesn't redraw.
    @State private var corrections = CorrectionClock()
    /// How many of the newest rows are drawn; Show Earlier Messages adds a page.
    @State private var shown = ConversationView.page
    /// Rows per page. A plain VStack, not a LazyVStack: lazy rows are measured as they scroll in, so
    /// their estimated heights kept changing under the follow-the-end scroll and SwiftUI re-measured
    /// forever inside one update (Notchling at 100% CPU, 5 Nov). Drawn rows have exact heights.
    static let page = 80

    static func == (a: Self, b: Self) -> Bool {
        a.conversation == b.conversation && a.pending == b.pending && a.outbox == b.outbox && a.live == b.live
            && a.expansion === b.expansion && a.pageOpen == b.pageOpen
    }

    private struct Row: Identifiable {
        let id: String
        let item: ChatItem?
        let day: Date?
    }

    /// Items with a day separator wherever the calendar day changes.
    private var rows: [Row] {
        var out: [Row] = []
        var lastDay: Date?
        let calendar = Calendar.current
        for item in conversation.items {
            if let date = item.date {
                let day = calendar.startOfDay(for: date)
                if day != lastDay { out.append(Row(id: "day-\(day.timeIntervalSince1970)", item: nil, day: day)); lastDay = day }
            }
            out.append(Row(id: item.id, item: item, day: nil))
        }
        return out
    }

    var body: some View {
        let allRows = self.rows
        let rows = Array(allRows.suffix(shown))
        let hidden = allRows.count - rows.count
        let lastToolID = conversation.items.last(where: { if case .tools = $0.kind { return true } else { return false } })?.id
        let lastID = conversation.items.last?.id
        // "Working" can outlast the turn: a background command (status "shell") keeps a session busy
        // after its reply. Then the row says so instead of "Thinking…" with a clock that never stops.
        let turnOver: Bool = { if case .turnEnd = conversation.items.last?.kind { return true } else { return false } }()
        VStack(spacing: 0) {
        if !conversation.tasks.isEmpty {
            TaskPanel(tasks: conversation.tasks, expansion: expansion).part("Task Panel", .topTrailing)
                        .padding(.horizontal, 16).padding(.top, 12)
                .frame(maxWidth: .infinity)
        }
        ScrollViewReader { proxy in
            GeometryReader { outer in
                ZStack(alignment: .bottomTrailing) {
                    ScrollView {
                        VStack(alignment: .leading, spacing: 14) {
                            if hidden > 0 {
                                Button("Show Earlier Messages") {
                                    // Keep the row you were reading in place as the page above it appears.
                                    let anchor = rows.first?.id
                                    shown += ConversationView.page
                                    if let anchor { DispatchQueue.main.async { proxy.scrollTo(anchor, anchor: .top) } }
                                }
                                .buttonStyle(.kit(.secondary))
                                .help("\(hidden) earlier")
                                .frame(maxWidth: .infinity)
                                .part("Show Earlier Button", .trailing)
                            }
                            ForEach(rows) { row in
                                Group {
                                    if let day = row.day {
                                        DaySeparator(date: day).part("Day Separator", .trailing)
                                    } else if let item = row.item {
                                        itemView(item, liveTools: live && !turnOver && item.id == lastToolID && item.id == lastID)
                                    }
                                }
                                .id(row.id)
                            }
                            // Only while it works: a queue left by a session that stopped never runs.
                            // The newest of these can be taken back to edit: the last waiting one, else the
                            // last queued one while nothing is on its way.
                            let sending = pending.map { !$0.landed(in: conversation) && $0.sending } ?? false
                            let editQueued = live && outbox.isEmpty && !sending
                            if live {
                                let queued = conversation.queued
                                ForEach(Array(queued.enumerated()), id: \.offset) { i, text in
                                    UserBubble(text: text, images: 0, date: nil, note: "Queued, sends when this turn ends",
                                               onEdit: editQueued && i == queued.count - 1 ? onEditLatest : nil)
                                        .part("Queued Bubble", .topTrailing)
                                }
                            }
                            if let pending, !pending.landed(in: conversation) {
                                UserBubble(text: pending.text, images: 0, date: nil, note: pending.sending ? "Sending…" : "Sent")
                                    .part("Sending Bubble", .topTrailing)
                                    .transition(.opacity.combined(with: .move(edge: .bottom)))
                            }
                            ForEach(Array(outbox.enumerated()), id: \.offset) { i, text in
                                UserBubble(text: text, images: 0, date: nil, note: "Waiting to send",
                                           onEdit: i == outbox.count - 1 ? onEditLatest : nil)
                                    .part("Waiting Bubble", .topTrailing)
                            }
                            if live && turnOver {
                                HStack(spacing: 8) {
                                    ProgressView().controlSize(.small)
                                    Text("A background command is still running").kitType(.meta).foregroundColor(KitInk.textMuted)
                                }
                            } else if live {
                                HStack(spacing: 8) {
                                    ProgressView().controlSize(.small)
                                    Text(conversation.activity ?? "Thinking…").kitType(.meta).foregroundColor(KitInk.textMuted).lineLimit(1)
                                    // Like Claude Code: time since your prompt, and tokens written since.
                                    if let start = conversation.turnStart {
                                        TimelineView(.periodic(from: .now, by: 1)) { context in
                                            Text("(" + ChatFormat.workStats(since: start, now: context.date, tokens: conversation.turnTokens) + ")")
                                                .kitType(.meta).foregroundColor(KitInk.textFaint).monospacedDigit()
                                        }
                                        .fixedSize()
                                    }
                                }
                            }
                            Color.clear.frame(height: 1).id("end")
                                .background(GeometryReader { g in
                                    Color.clear.preference(key: ScrollEdgesKey.self,
                                                           value: ScrollEdges(end: g.frame(in: .named("chatScroll")).minY))
                                })
                        }
                        .background(UserScrollWatcher { following in
                            if following != atBottom { withAnimation(KitMotion.hover) { atBottom = following } }
                        })
                                        .padding(.horizontal, 16).padding(.vertical, 20)
                        .frame(maxWidth: .infinity)
                    }
                    .coordinateSpace(name: "chatScroll")
                    .onPreferenceChange(ScrollEdgesKey.self) { edges in
                        // Off screen (a lazy row not drawn), the end counts as far below.
                        let near = (edges.end ?? .infinity) < outer.size.height + 4
                        // A message or step grew below while following: stay on the newest.
                        // At most one correction per 0.3 s: lazy rows are measured as they appear, so each scroll
                        // can move the end again, and an unbounded chain pins the main thread (the hangs of 4 Nov).
                        if atBottom && !near && Date() >= jumpingUntil {
                            corrections.run { if atBottom { proxy.scrollTo("end", anchor: .bottom) } }
                        }
                    }
                    if !atBottom {
                        Button { jump(proxy) } label: {
                            HStack(spacing: 6) {
                                Image(systemName: "arrow.down").font(.system(size: 11, weight: .semibold))
                                Text("Latest")
                            }
                        }
                        .buttonStyle(.kit(.secondary))
                        .shadow(color: KitInk.shadow, radius: 8, y: 3)
                        .padding(20)
                        .help("Jump to the newest message")
                        .transition(.opacity)
                    }
                }
            }
            .onAppear {
                // Twice: rows below the fold are measured lazily, so the first jump can fall short.
                DispatchQueue.main.async { proxy.scrollTo("end", anchor: .bottom) }
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.25) { proxy.scrollTo("end", anchor: .bottom) }
            }
            // The page panel opening or closing rewraps every message; stay at the newest.
            .onChange(of: pageOpen) { _ in
                guard atBottom else { return }
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) { proxy.scrollTo("end", anchor: .bottom) }
            }
            // Follow new messages only while you're at the bottom; never yank you from what you read.
            .onChange(of: rows.last?.id) { _ in
                if atBottom { jump(proxy) }
            }
            .onChange(of: pending) { _ in jump(proxy) }
            .onChange(of: outbox.count) { _ in jump(proxy) }
        }
        }
    }

    private func jump(_ proxy: ScrollViewProxy) {
        atBottom = true
        jumpingUntil = Date().addingTimeInterval(0.5)
        withAnimation(KitMotion.appear) { proxy.scrollTo("end", anchor: .bottom) }
    }

    @ViewBuilder private func itemView(_ item: ChatItem, liveTools: Bool) -> some View {
        switch item.kind {
        case .user(let text, let images):
            UserBubble(text: text, images: images, date: item.date, note: nil)
                .part(text.hasPrefix("! ") ? "Shell Command Bubble" : "Your Message", .topTrailing)
        case .assistant(let text):
            AssistantMessage(text: text, date: item.date).part("Claude's Reply")
        case .thinking(let seconds, let text):
            ThinkingRow(id: item.id, seconds: seconds, text: text, expansion: expansion).part("Thinking Row", .trailing)
        case .tools(let calls):
            ToolStepView(id: item.id, calls: calls, live: liveTools, expansion: expansion).part("Tool Steps", .topTrailing)
        case .turnEnd(let seconds, let model):
            TurnFooter(seconds: seconds, model: model).part("Turn Footer", .trailing)
        case .notice(let text, let symbol):
            NoticeRow(text: text, symbol: symbol).part("Notice", .leading)
        case .commandOutput(let text):
            CommandOutputView(text: text).part("Command Output", .topTrailing)
        }
    }
}

/// Runs at most one scroll correction per interval; a request that comes too soon waits for the
/// next slot (one waiting at a time), so the view still ends at the newest message.
private final class CorrectionClock {
    private var next = Date.distantPast
    private var waiting = false

    func run(_ action: @escaping () -> Void) {
        let now = Date()
        if now >= next {
            next = now.addingTimeInterval(0.3)
            DispatchQueue.main.async(execute: action)
        } else if !waiting {
            waiting = true
            DispatchQueue.main.asyncAfter(deadline: .now() + next.timeIntervalSince(now)) { [self] in
                waiting = false
                next = Date().addingTimeInterval(0.3)
                action()
            }
        }
    }
}

/// The conversation's end marker, in the scroll view's space; nil when not drawn.
private struct ScrollEdges: Equatable {
    var end: CGFloat?
}

private struct ScrollEdgesKey: PreferenceKey {
    static var defaultValue = ScrollEdges()
    static func reduce(value: inout ScrollEdges, nextValue: () -> ScrollEdges) {
        if let end = nextValue().end { value.end = end }
    }
}

/// Tells the conversation when you scroll it (wheel, trackpad, scroller), which from inside SwiftUI
/// looks the same as our own scrolls and as content growing below. Reports whether you're following
/// the end: scrolling up stops at once (before the scroll lands, so nothing snaps you back), and
/// reaching the bottom again resumes.
private struct UserScrollWatcher: NSViewRepresentable {
    let onScroll: (_ following: Bool) -> Void

    func makeNSView(context: Context) -> WatchView { WatchView() }

    func updateNSView(_ view: WatchView, context: Context) { view.onScroll = onScroll }

    final class WatchView: NSView {
        var onScroll: ((Bool) -> Void)?
        private var monitor: Any?
        private var observer: NSObjectProtocol?
        /// How close to the end counts as at the bottom.
        private let slack: CGFloat = 16

        override func hitTest(_ point: NSPoint) -> NSView? { nil }

        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            stop()
            guard window != nil else { return }
            monitor = NSEvent.addLocalMonitorForEvents(matching: .scrollWheel) { [weak self] event in
                self?.wheel(event)
                return event
            }
            // Dragging the scroller.
            observer = NotificationCenter.default.addObserver(forName: NSScrollView.didLiveScrollNotification, object: nil,
                                                              queue: .main) { [weak self] note in
                guard let self, let scroll = self.enclosingScrollView, note.object as? NSScrollView === scroll else { return }
                self.onScroll?(self.distanceFromEnd(scroll) < self.slack)
            }
        }

        private func wheel(_ event: NSEvent) {
            guard event.window === window, let scroll = enclosingScrollView,
                  scroll.bounds.contains(scroll.convert(event.locationInWindow, from: nil)),
                  let document = scroll.documentView, document.frame.height > scroll.contentView.bounds.height else { return }
            // Positive is toward older messages.
            if event.scrollingDeltaY > 0 { onScroll?(false) }
            DispatchQueue.main.async { [weak self] in
                guard let self else { return }
                self.onScroll?(self.distanceFromEnd(scroll) < self.slack)
            }
        }

        private func distanceFromEnd(_ scroll: NSScrollView) -> CGFloat {
            guard let document = scroll.documentView else { return 0 }
            let visible = scroll.contentView.bounds
            return document.isFlipped ? document.frame.height - visible.maxY : visible.minY
        }

        private func stop() {
            if let monitor { NSEvent.removeMonitor(monitor) }
            if let observer { NotificationCenter.default.removeObserver(observer) }
            monitor = nil
            observer = nil
        }

        deinit { stop() }
    }
}
