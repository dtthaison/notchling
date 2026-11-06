import AppKit
import Combine
import SwiftUI
import UniformTypeIdentifiers

struct SessionDetail: View {
    @ObservedObject var model: ChatModel
    @ObservedObject var notch: NotchModel
    let agent: Agent
    @State private var dropping = false

    var body: some View {
        VStack(spacing: 0) {
            // A fresh header and conversation per session (so switching back opens at the newest message
            // and the reply box takes focus), but the side panel below sits outside that identity: a page or
            // file open beside the chat stays as it is, scroll position and all, while you change session.
            header.id(agent.id)
            Rectangle().fill(KitInk.hairline).frame(height: 1)
            GeometryReader { geo in
            HStack(spacing: 0) {
                VStack(spacing: 0) {
                    if let sub = model.viewedSubagent {
                        SubagentBanner(subagent: sub) { model.subagentID = nil }.part("Subagent Banner", .bottom)
                        if let conversation = model.subConversation {
                            ConversationView(conversation: conversation, pending: nil, live: sub.isRunning,
                                             expansion: model.expansion, pageOpen: model.panelOpen)
                                .equatable()
                                .id(sub.id)
                                .part("Subagent Conversation", .bottom)
                        } else {
                            ProgressView().controlSize(.small).frame(maxWidth: .infinity, maxHeight: .infinity)
                        }
                    } else if let command = agent.command {
                        CommandJobView(agent: agent, command: command).part("Command Job", .top)
                    } else if model.conversation.items.isEmpty && model.pending[agent.id] == nil {
                        EmptyState(pose: agent.group == .working ? "look-2" : "sit-1", title: "No Messages Yet",
                                   message: agent.group == .working ? "The session is starting." : "Nothing in this session's transcript yet.")
                    } else {
                        ConversationView(conversation: model.conversation,
                                         pending: model.pending[agent.id],
                                         outbox: model.outbox[agent.id] ?? [],
                                         live: agent.group == .working,
                                         expansion: model.expansion,
                                         pageOpen: model.panelOpen,
                                         onEditLatest: { [weak model] in model?.editLatestQueued() })
                            .equatable()
                            .part("Conversation", .bottom)
                    }
                    if model.viewedSubagent == nil { bottom }
                }
                .id(agent.id)
                // Files and images dropped anywhere on the conversation go into the reply as paths, as in
                // Claude Code's terminal; Claude reads them from there.
                .onDrop(of: ChatModel.dropTypes, isTargeted: $dropping) { providers in
                    agent.canType && model.receiveDrop(providers, for: agent.id)
                }
                if let page = model.page {
                    PagePanel(model: model, page: page, maxWidth: max(320, geo.size.width - 380)).part("Page Panel", .topTrailing)
                        .transition(.move(edge: .trailing))
                } else if let file = model.file {
                    FilePanel(model: model, url: file, maxWidth: max(320, geo.size.width - 380)).part("File Panel", .topTrailing)
                        .transition(.move(edge: .trailing))
                } else if model.showInspector {
                    Rectangle().fill(KitInk.hairline).frame(width: 1)
                    SessionInspector(agent: agent, conversation: model.conversation,
                                     subagents: model.subagents, viewing: model.subagentID,
                                     openSubagent: { id in model.subagentID = id })
                        .part("Details Panel", .topTrailing)
                        .id(agent.id)
                        .frame(width: 250)
                        .transition(.move(edge: .trailing))
                }
            }
            }
        }
    }

    private var header: some View {
        HStack(spacing: 12) {
            // The status is the avatar's dot, as in the sidebar; the word is in its tooltip.
            KitAvatar(name: agent.name, size: 36, status: agent.group)
                .help(agent.label)
                .accessibilityLabel("\(agent.name), \(agent.label)")
                .part("Avatar", .bottomLeading)
            VStack(alignment: .leading, spacing: 4) {
                HStack(spacing: 6) {
                    ToolIcon(tool: SessionTool(agent.target), size: 18).part("Tool Icon", .leading)
                    Text(agent.name).kitType(.title).foregroundColor(KitInk.text).lineLimit(1)
                        .part("Session Name", .topTrailing)
                }
                HStack(spacing: 8) {
                    Text([agent.project, agent.host].filter { !$0.isEmpty }.joined(separator: " · "))
                        .kitType(.meta).foregroundColor(KitInk.textMuted).lineLimit(1)
                        .layoutPriority(1)
                        .part("Project Line", .bottomLeading)
                    if let account = model.account(for: agent) {
                        AccountMenu(model: model, agent: agent, account: account)
                            .part("Account Menu", .bottomLeading)
                    } else if case .codex = agent.target, let email = model.codex?.active?.email {
                        // Codex has one login: every Codex session runs on it.
                        Button { model.addAccount(.codex) } label: {
                            HStack(spacing: 4) {
                                Image(systemName: "person.crop.circle").font(.system(size: 10, weight: .semibold))
                                Text(email).lineLimit(1).truncationMode(.middle)
                                Image(systemName: "chevron.down").font(.system(size: 7, weight: .bold))
                            }
                            .kitType(.meta).foregroundColor(KitInk.textMuted)
                        }
                        .buttonStyle(.kitPlain)
                        .help("The Codex account. Click to switch or add one.")
                        .part("Codex Account Button", .bottomLeading)
                    }
                }
            }
            Spacer(minLength: 8)
            SessionMeters(context: model.conversation.facts.contextTokens,
                          window: ChatFormat.contextWindow(model: model.conversation.facts.model),
                          limits: model.limits(for: agent))
            // Icon only, so the line under the title has the room.
            Button { AgentActions.open(agent.target, cwd: agent.cwd) } label: {
                KitIconView(icon: ChatFormat.openIcon(agent), size: 13)
            }
            .buttonStyle(.kit(.ghost))
            .help("\(ChatFormat.openTitle(agent)): \(ChatFormat.openHelp(agent))")
            .accessibilityLabel(ChatFormat.openTitle(agent))
            .part("Open Button", .top)
            Button { withAnimation(KitMotion.appear) { model.showInspector.toggle() } } label: {
                Image(systemName: "sidebar.right").font(.system(size: 13, weight: .medium))
            }
            .buttonStyle(.kit(model.showInspector ? .secondary : .ghost))
            .help(model.showInspector ? "Hide the session details" : "Show the session details: branch, model, tokens, tasks, files changed")
            .part("Details Button", .bottomTrailing)
            PhoneButton(bridge: PhoneService.shared.bridge)
        }
        .padding(.horizontal, 16)
        .padding(.top, 14).padding(.bottom, 12)
        .frame(minHeight: 64)
        .part("Header", .bottom)
    }

    @ViewBuilder private var bottom: some View {
        VStack(spacing: 8) {
        if let error = model.accountErrors[agent.id] {
            AccountBanner(text: error, isError: true) { model.accountErrors[agent.id] = nil }.part("Account Banner", .trailing)
        } else if let note = model.accountNote(for: agent) {
            AccountBanner(text: note.text, isError: false) { model.dismissAccountNote(for: agent) }.part("Account Banner", .trailing)
        }
        Group {
            if let (tool, session) = agent.target.acp, let request = tool.runner.permission(session) {
                AntigravityPermissionCard(session: session, request: request, runner: tool.runner).part("Permission Card", .topTrailing)
            } else if let request = notch.permission(agent.id) {
                ChatPermissionCard(notch: notch, agent: agent, request: request).part("Permission Card", .topTrailing)
            } else if let question = notch.question(agent.id) {
                ChatQuestionCard(notch: notch, agent: agent, question: question).part("Question Card", .topTrailing)
            } else {
                switch agent.target {
                case .job where agent.command != nil:
                    Notice(text: "This background job runs a command, not a conversation, so it can't take replies.")
                        .part("Read-Only Notice", .trailing)
                case .job, .codex, .antigravity, .gemini:
                    Composer(model: model, agent: agent, dropping: dropping)
                case .process where agent.repliesViaHost:
                    Composer(model: model, agent: agent, dropping: dropping)
                case .process:
                    Notice(text: agent.host == "claude -p"
                           ? "This session runs headless (claude -p) and can't take replies."
                           : "Replies to this session go to its terminal. The terminal button at the top right brings it forward.")
                        .part("Read-Only Notice", .trailing)
                case .resume:
                    Notice(text: "This session has ended. Resume it in a terminal to keep going.").part("Read-Only Notice", .trailing)
                }
            }
        }
        }
        .padding(.horizontal, 16).padding(.bottom, 16).padding(.top, 4)
        .frame(maxWidth: .infinity)
    }
}
