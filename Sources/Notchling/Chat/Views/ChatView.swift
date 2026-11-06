import AppKit
import Combine
import SwiftUI
import UniformTypeIdentifiers

// MARK: - Views (parts and tokens from ChatKit.swift; spec in docs/chat-kit.md)

struct ChatView: View {
    @ObservedObject var model: ChatModel
    @ObservedObject var notch: NotchModel

    var body: some View {
        HStack(spacing: 0) {
            SessionSidebar(model: model, notch: notch)
                .part("Sidebar", .trailing)
                .frame(width: KitMetric.sidebarWidth)
                .background(KitInk.sidebar)
            Rectangle().fill(KitInk.hairline).frame(width: 1)
            Group {
                if let agent = model.selected {
                    SessionDetail(model: model, notch: notch, agent: agent).id(agent.id)
                } else {
                    // No session to read, but a page opened from search still shows on the right.
                    GeometryReader { geo in
                        HStack(spacing: 0) {
                            Group {
                                if notch.agents.isEmpty {
                                    EmptyState(pose: "doze-2", title: "No Sessions",
                                               message: "Sessions show up here when you run claude in a terminal or start a background session.",
                                               action: newSessionButton)
                                } else {
                                    EmptyState(pose: "wave-3", title: "Pick a Session",
                                               message: "Choose one on the left to read its conversation, or start a new one.",
                                               action: newSessionButton)
                                }
                            }
                            .frame(maxWidth: .infinity, maxHeight: .infinity)
                            if let page = model.page {
                                PagePanel(model: model, page: page, maxWidth: max(320, geo.size.width - 280)).part("Page Panel", .topTrailing)
                                    .transition(.move(edge: .trailing))
                            } else if let file = model.file {
                                FilePanel(model: model, url: file, maxWidth: max(320, geo.size.width - 280)).part("File Panel", .topTrailing)
                                    .transition(.move(edge: .trailing))
                            }
                        }
                    }
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(KitInk.window)
        }
        .ignoresSafeArea()
        .background(KitWindowChrome())
        .sheet(item: $model.terminal) { session in
            TerminalSheet(session: session) { model.closeTerminal() }
        }
        .background(EmptyView().sheet(isPresented: $model.accountSheet) {
            AccountsSheet(model: model)
        })
        .background(EmptyView().sheet(isPresented: $model.newSessionSheet) {
            NewSessionSheet(model: model)
        })
        .background(EmptyView().sheet(isPresented: $model.vaultSheet) {
            VaultSheet { model.vaultSheet = false }
        })
        .environment(\.openURL, OpenURLAction { model.open($0) })
        .environment(\.chatFolder, model.selected?.cwd)
        .environment(\.secondOpinion, model.selected.map { agent in
            SecondOpinionContext(store: model.secondOpinions, session: agent.id, folder: agent.cwd,
                                 open: { [weak model] run in model?.openDelegatedSession(run) })
        })
        .environment(\.runCommand, { [weak model] script, folder in model?.runCommand(script, in: folder) })
        .kitNoFocusRing()
        .frame(minWidth: 720, minHeight: 460)
    }

    private var newSessionButton: AnyView {
        AnyView(Button { model.newSessionSheet = true } label: {
            Label("New Session", systemImage: "plus")
        }
        .buttonStyle(.kit(.primary))
        .help("New session: pick a Claude Code or Codex account and a folder")
        .part("New Session Button", .bottom))
    }
}
