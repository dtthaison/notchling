import AppKit
import Combine
import SwiftUI
import UniformTypeIdentifiers

struct AccountMenu: View {
    @ObservedObject var model: ChatModel
    let agent: Agent
    let account: ChatModel.SessionAccount

    var body: some View {
        Button { popUp() } label: {
            HStack(spacing: 4) {
                Image(systemName: account.lane == nil ? "person.crop.circle" : "arrow.triangle.branch")
                    .font(.system(size: 10, weight: .semibold))
                Text(account.email).lineLimit(1).truncationMode(.middle)
                if account.lane != nil { Text("· lane").foregroundColor(KitInk.textFaint) }
                Image(systemName: "chevron.down").font(.system(size: 7, weight: .bold))
            }
            .kitType(.meta).foregroundColor(KitInk.textMuted)
            .contentShape(Rectangle())
        }
        .buttonStyle(.kitPlain)
        .pointerCursor()
        .help(account.lane.map { "This session runs in the \($0.name) lane, on \($0.email). Click to switch it." }
              ?? "This session runs on the main login. Click to switch it.")
    }

    /// One question per menu: which account this session uses. Picking another moves just this session;
    /// where it then runs (the main login, a lane, a new lane) is Notchling's business, not a choice.
    /// Moving everything is one submenu below. Switching the main login or a whole lane lives in the
    /// menu bar (accounts list, Lanes), and new sessions start from the sidebar's +.
    func popUp() {
        let menu = NSMenu()
        menu.autoenablesItems = false
        func note(_ title: String) {
            let item = NSMenuItem(title: title, action: nil, keyEquivalent: "")
            item.isEnabled = false
            menu.addItem(item)
        }
        func header(_ title: String) {
            if #available(macOS 14, *) { menu.addItem(.sectionHeader(title: title)) } else { note(title) }
        }
        let here = account.lane?.accountID
        let targets = model.sessionTargets
        header("This Session's Account")
        switch agent.target {
        case .job where agent.command == nil && agent.transcriptPath != nil:
            for target in targets {
                let current = target.accountID == here
                let item = ActionMenuItem(target.title) { [model, agent] in
                    guard !current else { return }
                    model.moveSession(agent, to: target.accountID, title: target.title)
                }
                item.state = current ? .on : .off
                menu.addItem(item)
            }
            note("Picking one moves only this session; the conversation continues there.")
        default:
            // A terminal session keeps the account it was started with.
            let item = NSMenuItem(title: account.email, action: nil, keyEquivalent: "")
            item.state = .on
            item.isEnabled = false
            menu.addItem(item)
            if case .process = agent.target {
                let others = targets.filter { $0.accountID != nil }
                if !others.isEmpty {
                    let open = NSMenuItem(title: "New Terminal Session On", action: nil, keyEquivalent: "")
                    let sub = NSMenu()
                    for target in others {
                        sub.addItem(ActionMenuItem(target.title) { [model, agent] in
                            if let id = target.accountID { model.newTerminalSession(for: agent, on: id) }
                        })
                    }
                    open.submenu = sub
                    menu.addItem(open)
                }
            }
        }
        if let accounts = model.accounts {
            let lanes = LaneStore.all()
            let all = targets.filter { target in
                !MoveAllPlan.make(agents: model.notch.agents, lanes: lanes, activeID: accounts.activeID, target: target.accountID).isEmpty
            }
            if !all.isEmpty {
                menu.addItem(.separator())
                let item = NSMenuItem(title: "Move All Sessions To", action: nil, keyEquivalent: "")
                let sub = NSMenu()
                for target in all {
                    sub.addItem(ActionMenuItem(target.title) { [model, agent] in
                        model.moveAll(to: target.accountID, email: target.title, from: agent)
                    })
                }
                item.submenu = sub
                menu.addItem(item)
            }
        }
        if model.accounts != nil {
            menu.addItem(.separator())
            menu.addItem(ActionMenuItem("Add Account…") { [model] in model.addAccount() })
            if model.codex?.isAvailable == true {
                menu.addItem(ActionMenuItem("Codex Accounts…") { [model] in model.addAccount(.codex) })
            }
        }
        menu.popUp(positioning: nil, at: NSEvent.mouseLocation, in: nil)
    }
}

/// A menu item that runs a closure (our own selector: macOS 26 adds icons to standard actions).
final class ActionMenuItem: NSMenuItem {
    private let run: () -> Void

    init(_ title: String, run: @escaping () -> Void) {
        self.run = run
        super.init(title: title, action: #selector(fire), keyEquivalent: "")
        target = self
    }

    required init(coder: NSCoder) { fatalError("not used") }

    @objc private func fire() { run() }
}

/// A limit switch that moved this session, or an account action that failed.
struct AccountBanner: View {
    let text: String
    let isError: Bool
    let dismiss: () -> Void

    var body: some View {
        HStack(spacing: 10) {
            Image(systemName: isError ? "exclamationmark.triangle" : "arrow.triangle.swap")
                .font(.system(size: 12, weight: .medium))
                .foregroundColor(isError ? KitInk.needs : KitInk.accentText)
            Text(text).kitType(.label).foregroundColor(KitInk.text).fixedSize(horizontal: false, vertical: true)
            Spacer(minLength: 4)
            Button(action: dismiss) { Image(systemName: "xmark").font(.system(size: 10, weight: .semibold)) }
                .buttonStyle(.kit(.ghost)).help("Dismiss")
        }
        .padding(.leading, 14).padding(.trailing, 6).padding(.vertical, 6)
        .kitCard(radius: KitMetric.radiusMd, fill: KitInk.accentWash)
    }
}
