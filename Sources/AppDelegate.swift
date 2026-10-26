import AppKit
import ServiceManagement

final class AppDelegate: NSObject, NSApplicationDelegate, NSMenuDelegate {
    private let awake = AwakeController()
    private let monitor = AgentMonitor()
    private let notch = NotchController()
    private let accounts = AccountStore()
    private let antigravity = AntigravityStore()
    private let claudeDesktop = ClaudeDesktopStore()
    private let cost = CostTracker()
    private let settings = SettingsModel()
    private lazy var accountsWindow = AccountsWindowController(accounts: accounts, antigravity: antigravity, desktop: claudeDesktop, cost: cost, settings: settings)
    private var statusItem: NSStatusItem!
    private var agents: [Agent] = []
    private var lastGroups: [String: AgentGroup]?
    private var timer: Timer?
    private var seenRequests = Set<String>()
    private var usageTimer: Timer?
    /// Rows in the open menu, updated in place when limits arrive.
    private var accountRows: [String: AccountRowView] = [:]
    private var costChart: CostChartView?

    /// Completed agents the user cleared, keyed by id, with the time they were cleared at.
    private var dismissed: [String: Double] {
        get { UserDefaults.standard.dictionary(forKey: "dismissed") as? [String: Double] ?? [:] }
        set { UserDefaults.standard.set(newValue, forKey: "dismissed") }
    }

    /// Agents hidden from a row's menu, keyed by id, with the group they were in. Each comes back
    /// once it moves to another group (finishes, or starts waiting on you).
    private var hidden: [String: Int] {
        get { UserDefaults.standard.dictionary(forKey: "hiddenAgents") as? [String: Int] ?? [:] }
        set { UserDefaults.standard.set(newValue, forKey: "hiddenAgents") }
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.squareLength)
        statusItem.button?.imagePosition = .imageLeading
        let menu = NSMenu()
        menu.delegate = self
        statusItem.menu = menu

        HookBridge.beat()
        // First launch with this version: turn on answering prompts from the notch.
        if UserDefaults.standard.object(forKey: "answerFromNotch") == nil {
            setAnswerFromNotch(true)
        } else if answerFromNotch, !HookBridge.isInstalled {
            setAnswerFromNotch(true)
        }

        refresh()
        if showNotch { notch.show() }
        timer = Timer.scheduledTimer(withTimeInterval: 3, repeats: true) { [weak self] _ in self?.refresh() }

        settings.apply = { [weak self] in self?.applySetting($0) }
        settings.reload = { [weak self] in self?.reloadSettings() }
        accounts.changed = { [weak self] in self?.updateUsageViews() }
        antigravity.changed = { [weak self] in self?.updateButton() }
        antigravity.sync()
        claudeDesktop.changed = { [weak self] in self?.updateButton() }
        claudeDesktop.sync()
        cost.changed = { [weak self] in self?.updateUsageViews() }
        accounts.sync()
        cost.refresh()
        usageTimer = Timer.scheduledTimer(withTimeInterval: 60, repeats: true) { [weak self] _ in
            self?.accounts.sync()
            self?.antigravity.sync()
            self?.claudeDesktop.sync()
            self?.cost.refresh()
        }
    }

    private var answerFromNotch: Bool {
        UserDefaults.standard.bool(forKey: "answerFromNotch")
    }

    private func setAnswerFromNotch(_ on: Bool) {
        do {
            try HookBridge.setInstalled(on)
            UserDefaults.standard.set(on, forKey: "answerFromNotch")
        } catch {
            let alert = NSAlert()
            alert.messageText = "Couldn't update Claude Code's settings"
            alert.informativeText = error.localizedDescription
            alert.runModal()
        }
    }

    private var showNotch: Bool {
        get { !UserDefaults.standard.bool(forKey: "hideNotch") }
        set { UserDefaults.standard.set(!newValue, forKey: "hideNotch") }
    }

    /// The app used to be called Awake (bundle local.awake.menubar). Carry its settings over once.
    /// Runs before the delegate exists, since its parts read settings as they're created.
    static func migrateFromAwake() {
        let defaults = UserDefaults.standard
        guard !defaults.bool(forKey: "migratedFromAwake"),
              let old = UserDefaults(suiteName: "local.awake.menubar")?.dictionaryRepresentation() else { return }
        for key in ["mode", "active", "keepDisplayOn", "hideNotch", "answerFromNotch", "showPet", "notchWidth", "dismissed", "hiddenAgents", "accounts", "untrackedCostAccount", "antigravityAccounts", "claudeDesktopAccounts"] {
            if let value = old[key], defaults.object(forKey: key) == nil { defaults.set(value, forKey: key) }
        }
        defaults.set(true, forKey: "migratedFromAwake")
    }

    func applicationWillTerminate(_ notification: Notification) {
        awake.stop()
    }

    // MARK: - Polling

    private func refresh() {
        HookBridge.beat()
        let cleared = dismissed
        let requests = Dictionary(HookBridge.pendingRequests().map { ($0.sessionID, $0) }, uniquingKeysWith: { _, newer in newer })
        agents = monitor.poll().filter { a in
            guard a.group == .completed, let at = cleared[a.id] else { return true }
            return a.since.timeIntervalSince1970 > at
        }.map { a in
            // A prompt held by a hook means it's waiting on you, whatever the session file says.
            guard let r = requests[a.id] else { return a }
            var held = a
            held.group = .needsInput
            held.label = r.question != nil ? "Asks you" : "Needs permission"
            return held
        }
        let wasHidden = hidden
        let stillHidden = wasHidden.filter { id, group in agents.contains { $0.id == id && $0.group.rawValue == group } }
        agents.removeAll { stillHidden[$0.id] != nil }
        if stillHidden != wasHidden { hidden = stillHidden }

        let fresh = requests.values.filter { !seenRequests.contains($0.id) }
        seenRequests = Set(requests.values.map(\.id))
        notch.model.requests = requests
        notch.model.agents = agents
        notch.model.onClearCompleted = { [weak self] in self?.clearCompleted() }
        notch.model.onHide = { [weak self] in self?.hide($0) }
        notch.model.onRemove = { [weak self] in self?.remove($0) }
        if let request = fresh.first, let agent = agents.first(where: { $0.id == request.sessionID }), notch.isVisible {
            NSSound(named: "Tink")?.play()
            notch.model.peek(agent)
        }
        notch.model.refreshSnapshots { [weak self] old, new in self?.peekTransitions(old: old, new: new) }
        awake.apply(agentsWorking: agents.contains { $0.group == .working })
        updateButton()
        if accountsWindow.isVisible { reloadSettings() }
    }

    private func updateButton() {
        guard let button = statusItem.button else { return }
        let on = awake.isActive
        let image = NSImage(systemSymbolName: on ? "cup.and.saucer.fill" : "cup.and.saucer",
                            accessibilityDescription: on ? "Notchling: keeping awake" : "Notchling")
        image?.isTemplate = true
        button.image = image

        button.title = ""
        var tip = "Keep awake: \(awake.mode.title)\(on ? " (on now)" : "")"
        if let account = accounts.active, let usage = accounts.usage[account.id] {
            let parts = [usage.session.map { "5h \(Int($0.percent.rounded()))%" }, usage.week.map { "week \(Int($0.percent.rounded()))%" }]
            tip += "\n\(account.email): " + parts.compactMap { $0 }.joined(separator: ", ")
        }
        button.toolTip = tip
    }

    /// Peeks out of the notch when an agent replies (starts needing input or finishes) or asks a question.
    /// The first poll only sets the baseline.
    private func peekTransitions(old: [String: Snapshot], new: [String: Snapshot]) {
        let current = Dictionary(agents.map { ($0.id, $0.group) }, uniquingKeysWith: { a, _ in a })
        defer { lastGroups = current }
        guard let previous = lastGroups else { return }

        let asked = agents.filter { agent in
            guard let q = new[agent.id]?.question else { return false }
            return old[agent.id]?.question?.toolUseID != q.toolUseID
        }
        let changed = agents.filter { agent in
            guard let before = previous[agent.id] else { return false }
            return before != agent.group && agent.group != .working
        }
        // A question beats someone waiting on you, which beats something finishing.
        guard let agent = asked.first ?? changed.first(where: { $0.group == .needsInput }) ?? changed.first else { return }
        if agent.group == .needsInput || !asked.isEmpty { NSSound(named: "Tink")?.play() }
        if notch.isVisible { notch.model.peek(agent) }
    }

    // MARK: - Menu

    func menuNeedsUpdate(_ menu: NSMenu) {
        refresh()
        menu.removeAllItems()
        addUsageItems(to: menu)
        menu.addItem(.separator())

        menu.addItem(header("Keep Mac Awake"))
        for mode in AwakeMode.allCases {
            let item = action(mode.title, #selector(setMode(_:)))
            item.representedObject = mode.rawValue
            item.state = awake.mode == mode ? .on : .off
            menu.addItem(item)
        }
        let display = action("Keep Display On", #selector(toggleDisplay))
        display.state = awake.keepDisplayOn ? .on : .off
        menu.addItem(display)

        menu.addItem(.separator())
        let notchItem = action("Show Agents in Notch", #selector(toggleNotch))
        notchItem.state = showNotch ? .on : .off
        menu.addItem(notchItem)
        let widthMenu = NSMenu()
        for width in NotchWidth.allCases {
            let item = action(width.title, #selector(setWidth(_:)))
            item.representedObject = width.rawValue
            item.state = notch.model.width == width ? .on : .off
            widthMenu.addItem(item)
        }
        let widthItem = NSMenuItem(title: "Notch Width", action: nil, keyEquivalent: "")
        widthItem.submenu = widthMenu
        menu.addItem(widthItem)
        let petItem = action("Animate Pet", #selector(togglePet))
        petItem.state = notch.model.showPet ? .on : .off
        menu.addItem(petItem)
        let answerItem = action("Answer Prompts from the Notch", #selector(toggleAnswer))
        answerItem.state = answerFromNotch && HookBridge.isInstalled ? .on : .off
        answerItem.toolTip = "Adds a hook to Claude Code so questions and permission prompts can be answered here."
        menu.addItem(answerItem)
        let login = action("Launch at Login", #selector(toggleLogin))
        login.state = SMAppService.mainApp.status == .enabled ? .on : .off
        menu.addItem(login)
        // Its own action, not terminate(_:): macOS 26 gives standard actions an icon, which shifts
        // every item in the group to the right.
        let quit = action("Quit Notchling", #selector(quit))
        quit.keyEquivalent = "q"
        menu.addItem(quit)
    }

    // MARK: - Accounts and cost

    private func addUsageItems(to menu: NSMenu) {
        if accounts.lastSync.map({ Date().timeIntervalSince($0) > 30 }) ?? true { accounts.sync() }
        if cost.scanned.map({ Date().timeIntervalSince($0) > 30 }) ?? true { cost.refresh() }

        // Only the account in use here; the others are under Switch Account.
        menu.addItem(header("Claude Code"))
        accountRows = [:]
        if let account = accounts.active {
            let row = AccountRowView(account: account, usage: accounts.usage[account.id], isActive: true)
            row.onClick = { [weak self] in self?.manageAccounts() }
            let item = action(account.email, #selector(manageAccounts))
            item.view = row
            accountRows[account.id] = row
            menu.addItem(item)
        } else {
            menu.addItem(disabled("Not signed in to Claude Code"))
        }
        let switchItem = NSMenuItem(title: "Switch Account", action: nil, keyEquivalent: "")
        switchItem.submenu = switchMenu()
        menu.addItem(switchItem)
        let costItem = NSMenuItem(title: costTitle, action: nil, keyEquivalent: "")
        costItem.submenu = costMenu()
        menu.addItem(costItem)

        addDesktopItems(to: menu)
        addAntigravityItems(to: menu)

        menu.addItem(.separator())
        menu.addItem(action("Manage Accounts…", #selector(manageAccounts)))
        let settingsItem = action("Settings…", #selector(showSettings))
        settingsItem.keyEquivalent = ","
        menu.addItem(settingsItem)
    }

    /// The account Claude Desktop is on, and a Switch Account submenu for the rest.
    private func addDesktopItems(to menu: NSMenu) {
        guard claudeDesktop.isAvailable else { return }
        claudeDesktop.sync()
        menu.addItem(.separator())
        menu.addItem(header("Claude Desktop"))
        if claudeDesktop.adding {
            menu.addItem(disabled("Waiting for you to sign in…"))
            menu.addItem(action("Cancel Add Account", #selector(cancelDesktopAdd)))
            return
        }
        if let uuid = claudeDesktop.activeUuid {
            let email = accounts.accounts.first { $0.id.hasPrefix(uuid) }?.email ?? "Signed in"
            let item = action(email, #selector(manageAccounts))
            item.state = .on
            menu.addItem(item)
        } else {
            menu.addItem(disabled("Not signed in to Claude Desktop"))
        }
        let switchItem = NSMenuItem(title: "Switch Account", action: nil, keyEquivalent: "")
        switchItem.submenu = desktopSwitchMenu()
        menu.addItem(switchItem)
    }

    @objc private func cancelDesktopAdd() { claudeDesktop.cancelAdd() }

    /// The Google account Antigravity is on, and the others to switch to.
    private func addAntigravityItems(to menu: NSMenu) {
        guard antigravity.isAvailable else { return }
        antigravity.sync()
        menu.addItem(.separator())
        menu.addItem(header("Antigravity"))
        if antigravity.adding {
            menu.addItem(disabled("Waiting for you to sign in…"))
            menu.addItem(action("Cancel Add Account", #selector(cancelAntigravityAdd)))
            return
        }
        if let account = antigravity.active {
            let row = AntigravityRowView(account: account, usage: antigravity.usage[account.id])
            row.onClick = { [weak self] in self?.manageAntigravity() }
            let item = action(account.email, #selector(manageAntigravity))
            item.view = row
            menu.addItem(item)
        } else {
            menu.addItem(disabled("Not signed in to Antigravity"))
        }
        let switchItem = NSMenuItem(title: "Switch Account", action: nil, keyEquivalent: "")
        let submenu = NSMenu()
        for account in antigravity.accounts {
            let item = action(account.email, #selector(switchAntigravityItem(_:)))
            item.representedObject = account.id
            item.state = account.id == antigravity.activeID ? .on : .off
            if let plan = account.plan { item.attributedTitle = accountTitle(account.email, detail: [plan]) }
            submenu.addItem(item)
        }
        if !antigravity.accounts.isEmpty { submenu.addItem(.separator()) }
        submenu.addItem(action("Add Account…", #selector(manageAntigravity)))
        switchItem.submenu = submenu
        menu.addItem(switchItem)
    }

    /// Every saved account with its plan and limits, then Add Account.
    private func switchMenu() -> NSMenu {
        let menu = NSMenu()
        for account in accounts.accounts {
            let item = action(account.email, #selector(switchAccountItem(_:)))
            item.representedObject = account.id
            item.state = account.id == accounts.activeID ? .on : .off
            var detail = [account.plan]
            if account.needsLogin {
                detail.append("signed out")
            } else if let usage = accounts.usage[account.id] {
                if let s = usage.session { detail.append("5h \(Int(s.percent.rounded()))%") }
                if let w = usage.week { detail.append("week \(Int(w.percent.rounded()))%") }
            }
            item.attributedTitle = accountTitle(account.email, detail: detail)
            menu.addItem(item)
        }
        if !accounts.accounts.isEmpty { menu.addItem(.separator()) }
        menu.addItem(action("Add Account…", #selector(addAccount)))
        return menu
    }

    /// The same accounts, switching Claude Desktop instead of Claude Code. Only accounts Notchling has
    /// captured a desktop login for can be switched to; the rest are a hint to open them in Desktop first.
    private func desktopSwitchMenu() -> NSMenu {
        claudeDesktop.sync()
        let menu = NSMenu()
        menu.autoenablesItems = false
        if claudeDesktop.adding {
            menu.addItem(disabled("Waiting for you to sign in…"))
            return menu
        }
        // Only accounts actually in Desktop (the active one, or ones Notchling has captured).
        for account in accounts.accounts {
            let uuid = String(account.id.prefix { $0 != ":" })
            let active = claudeDesktop.activeUuid == uuid
            guard active || claudeDesktop.saved.contains(uuid) else { continue }
            let item = action(account.email, #selector(switchDesktopItem(_:)))
            item.representedObject = uuid
            item.state = active ? .on : .off
            menu.addItem(item)
        }
        if menu.items.contains(where: { $0.action != nil }) { menu.addItem(.separator()) }
        menu.addItem(action("Add Account…", #selector(addDesktopAccount)))
        return menu
    }

    /// Opens Claude Desktop signed out so the user can sign in to another account there.
    @objc private func addDesktopAccount() {
        if ClaudeDesktopStore.isRunning {
            let alert = NSAlert()
            alert.messageText = "Open Claude Desktop signed out?"
            alert.informativeText = "Claude Desktop quits and reopens at the sign-in screen. Sign in to the account "
                + "you want to add; Notchling saves it. Your current account is kept and you can switch back to it."
            alert.addButton(withTitle: "Continue")
            alert.addButton(withTitle: "Cancel")
            NSApp.activate(ignoringOtherApps: true)
            guard alert.runModal() == .alertFirstButtonReturn else { return }
        }
        claudeDesktop.beginAdd { error in
            guard let error else { return }
            let alert = NSAlert()
            alert.messageText = "Couldn't open Claude Desktop to add an account"
            alert.informativeText = error.localizedDescription
            NSApp.activate(ignoringOtherApps: true)
            alert.runModal()
        }
    }

    /// An account's email in menu font, then its plan and limits in small secondary text.
    private func accountTitle(_ email: String, detail: [String]) -> NSAttributedString {
        let title = NSMutableAttributedString(string: email, attributes: [.font: NSFont.menuFont(ofSize: 0)])
        title.append(NSAttributedString(string: "   " + detail.joined(separator: " · "), attributes: [
            .font: NSFont.menuFont(ofSize: NSFont.smallSystemFontSize), .foregroundColor: NSColor.secondaryLabelColor,
        ]))
        return title
    }

    private var costTitle: String {
        cost.scanned == nil ? "Claude Token Cost" : "Claude Token Cost Today: \(CostChartView.dollars(cost.summary.today))"
    }

    private func costMenu() -> NSMenu {
        let menu = NSMenu()
        menu.autoenablesItems = false
        let summary = cost.summary
        let chart = CostChartView(summary: summary)
        costChart = chart
        let chartItem = NSMenuItem()
        chartItem.view = chart
        menu.addItem(chartItem)
        if cost.scanned == nil { menu.addItem(disabled("Reading transcripts…")) }
        if summary.accounts.count > 1 || summary.accounts.first?.id.isEmpty == false {
            menu.addItem(header("By Account, 30 Days"))
            for row in summary.accounts {
                let name = row.id.isEmpty ? "Before Tracking" : accounts.accounts.first { $0.id == row.id }?.email ?? "Removed Account"
                menu.addItem(valueItem(name, row.cost))
            }
        }
        if summary.channels.count > 1 {
            menu.addItem(header("By Channel, 30 Days"))
            summary.channels.forEach { menu.addItem(valueItem($0.name, $0.cost)) }
        }
        if !summary.models.isEmpty {
            menu.addItem(header("By Model, 30 Days"))
            summary.models.prefix(6).forEach { menu.addItem(valueItem($0.name, $0.cost)) }
        }
        if !summary.projects.isEmpty {
            menu.addItem(header("By Project, 30 Days"))
            summary.projects.prefix(8).forEach { menu.addItem(valueItem($0.name, $0.cost)) }
        }
        menu.addItem(.separator())
        let details = action("Show Details…", #selector(showCost))
        menu.addItem(details)
        menu.addItem(disabled("At API prices, from Claude Code and Desktop transcripts"))
        return menu
    }

    /// "Opus 5.5 ……… $512.30" with the amount right-aligned.
    private func valueItem(_ name: String, _ value: Double) -> NSMenuItem {
        let item = NSMenuItem(title: name, action: nil, keyEquivalent: "")
        let style = NSMutableParagraphStyle()
        style.tabStops = [NSTextTab(textAlignment: .right, location: 260)]
        item.attributedTitle = NSAttributedString(string: "\(name)\t\(CostChartView.dollars(value))", attributes: [
            .paragraphStyle: style, .font: NSFont.menuFont(ofSize: 0),
        ])
        return item
    }

    /// Limits or cost arrived while the menu may be open: redraw what's on screen.
    private func updateUsageViews() {
        for (id, row) in accountRows {
            guard let account = accounts.accounts.first(where: { $0.id == id }) else { continue }
            row.account = account
            row.isActive = id == accounts.activeID
            row.usage = accounts.usage[id]
            row.relayout()
        }
        costChart?.summary = cost.summary
        updateButton()
    }

    @objc private func switchAccountItem(_ sender: NSMenuItem) {
        if let id = sender.representedObject as? String { switchAccount(id) }
    }

    private func switchAccount(_ id: String) {
        guard id != accounts.activeID else { return }
        accounts.switchTo(id) { error in
            guard let error else { return }
            let alert = NSAlert()
            alert.messageText = "Couldn't switch accounts"
            alert.informativeText = error.localizedDescription
            NSApp.activate(ignoringOtherApps: true)
            alert.runModal()
        }
    }

    @objc private func switchDesktopItem(_ sender: NSMenuItem) {
        guard let uuid = sender.representedObject as? String, uuid != claudeDesktop.activeUuid else { return }
        if ClaudeDesktopStore.isRunning {
            let alert = NSAlert()
            alert.messageText = "Restart Claude Desktop?"
            alert.informativeText = "Claude Desktop quits and opens again on the other account."
            alert.addButton(withTitle: "Restart")
            alert.addButton(withTitle: "Cancel")
            NSApp.activate(ignoringOtherApps: true)
            guard alert.runModal() == .alertFirstButtonReturn else { return }
        }
        claudeDesktop.switchTo(uuid) { error in
            guard let error else { return }
            let alert = NSAlert()
            alert.messageText = "Couldn't switch Claude Desktop"
            alert.informativeText = error.localizedDescription
            NSApp.activate(ignoringOtherApps: true)
            alert.runModal()
        }
    }

    @objc private func cancelAntigravityAdd() { antigravity.cancelAdd() }

    @objc private func switchAntigravityItem(_ sender: NSMenuItem) {
        guard let id = sender.representedObject as? String, id != antigravity.activeID else { return }
        if AntigravityStore.isRunning {
            let alert = NSAlert()
            alert.messageText = "Restart Antigravity?"
            alert.informativeText = "Antigravity quits and opens again on the other account. Agents running in it stop."
            alert.addButton(withTitle: "Restart")
            alert.addButton(withTitle: "Cancel")
            NSApp.activate(ignoringOtherApps: true)
            guard alert.runModal() == .alertFirstButtonReturn else { return }
        }
        antigravity.switchTo(id) { error in
            guard let error else { return }
            let alert = NSAlert()
            alert.messageText = "Couldn't switch Antigravity accounts"
            alert.informativeText = error.localizedDescription
            NSApp.activate(ignoringOtherApps: true)
            alert.runModal()
        }
    }

    @objc private func manageAntigravity() {
        accountsWindow.show(.accounts, provider: .antigravity)
    }

    /// Signs in from the Claude Accounts window; Claude Code stays on the account it's using.
    @objc private func addAccount() {
        accountsWindow.show(.accounts, provider: .claudeCode, signIn: true)
    }

    @objc private func manageAccounts() {
        accountsWindow.show(.accounts, provider: .claudeCode)
    }

    @objc private func showCost() {
        accountsWindow.show(.cost)
    }

    @objc private func showSettings() {
        accountsWindow.show(.settings)
    }

    // MARK: - Settings tab

    /// Copies the real settings into the Settings tab, touching only what changed.
    private func reloadSettings() {
        let login = SMAppService.mainApp.status == .enabled
        let answer = answerFromNotch && HookBridge.isInstalled
        settings.load { s in
            if s.awakeMode != awake.mode { s.awakeMode = awake.mode }
            if s.keepDisplayOn != awake.keepDisplayOn { s.keepDisplayOn = awake.keepDisplayOn }
            if s.showNotch != showNotch { s.showNotch = showNotch }
            if s.notchWidth != notch.model.width { s.notchWidth = notch.model.width }
            if s.animatePet != notch.model.showPet { s.animatePet = notch.model.showPet }
            if s.answerFromNotch != answer { s.answerFromNotch = answer }
            if s.launchAtLogin != login { s.launchAtLogin = login }
        }
    }

    private func applySetting(_ change: SettingsModel.Change) {
        switch change {
        case .awakeMode(let mode):
            awake.mode = mode
            refresh()
        case .keepDisplayOn(let on):
            if awake.keepDisplayOn != on { toggleDisplay() }
        case .showNotch(let on):
            if showNotch != on { toggleNotch() }
        case .notchWidth(let width):
            notch.model.width = width
        case .animatePet(let on):
            notch.model.showPet = on
        case .answerFromNotch(let on):
            setAnswerFromNotch(on)
        case .launchAtLogin(let on):
            if (SMAppService.mainApp.status == .enabled) != on { toggleLogin() }
        }
    }

    private func header(_ title: String) -> NSMenuItem {
        if #available(macOS 14, *) { return .sectionHeader(title: title) }
        return disabled(title)
    }

    private func disabled(_ title: String) -> NSMenuItem {
        let item = NSMenuItem(title: title, action: nil, keyEquivalent: "")
        item.isEnabled = false
        return item
    }

    private func action(_ title: String, _ selector: Selector) -> NSMenuItem {
        let item = NSMenuItem(title: title, action: selector, keyEquivalent: "")
        item.target = self
        return item
    }

    // MARK: - Actions

    @objc private func quit() {
        NSApp.terminate(nil)
    }

    @objc private func setWidth(_ sender: NSMenuItem) {
        if let raw = sender.representedObject as? String, let width = NotchWidth(rawValue: raw) {
            notch.model.width = width
        }
    }

    @objc private func togglePet() {
        notch.model.showPet.toggle()
    }

    @objc private func toggleAnswer() {
        setAnswerFromNotch(!(answerFromNotch && HookBridge.isInstalled))
    }

    @objc private func toggleNotch() {
        showNotch.toggle()
        if showNotch { notch.show() } else { notch.hide() }
    }

    private func hide(_ agent: Agent) {
        if agent.group == .completed {
            dismissed[agent.id] = Date().timeIntervalSince1970
        } else {
            hidden[agent.id] = agent.group.rawValue
        }
        refresh()
    }

    /// Stops a background agent and deletes its session and worktree with `claude rm`, after asking.
    private func remove(_ agent: Agent) {
        guard case .job(let id) = agent.target else { return }
        let alert = NSAlert()
        alert.messageText = "Remove “\(agent.name)”?"
        alert.informativeText = "This stops the agent if it's running and deletes its session and worktree, "
            + "like `claude rm \(id)`. Unpushed commits are kept unless you remove it from the terminal."
        alert.addButton(withTitle: "Remove")
        alert.addButton(withTitle: "Cancel")
        alert.buttons.first?.hasDestructiveAction = true
        NSApp.activate(ignoringOtherApps: true)
        guard alert.runModal() == .alertFirstButtonReturn else { return }

        DispatchQueue.global(qos: .userInitiated).async {
            let process = Process()
            process.executableURL = URL(fileURLWithPath: ReplySender.claudePath)
            process.arguments = ["rm", id]
            let pipe = Pipe()
            process.standardOutput = pipe
            process.standardError = pipe
            var output = ""
            do {
                try process.run()
                output = String(data: pipe.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8) ?? ""
                process.waitUntilExit()
            } catch {
                output = error.localizedDescription
            }
            let failed = process.isRunning || process.terminationStatus != 0
            DispatchQueue.main.async {
                self.refresh()
                guard failed else { return }
                let note = NSAlert()
                note.messageText = "Couldn't remove “\(agent.name)”"
                note.informativeText = output.trimmingCharacters(in: .whitespacesAndNewlines)
                NSApp.activate(ignoringOtherApps: true)
                note.runModal()
            }
        }
    }

    @objc private func clearCompleted() {
        var cleared = dismissed
        let now = Date().timeIntervalSince1970
        for agent in agents where agent.group == .completed { cleared[agent.id] = now }
        // Forget entries old enough that they'd be hidden anyway.
        dismissed = cleared.filter { now - $0.value < 7 * 86400 }
        refresh()
    }

    @objc private func setMode(_ sender: NSMenuItem) {
        guard let raw = sender.representedObject as? String, let mode = AwakeMode(rawValue: raw) else { return }
        awake.mode = mode
        refresh()
    }

    @objc private func toggleDisplay() {
        awake.keepDisplayOn.toggle()
        refresh()
    }

    @objc private func toggleLogin() {
        let service = SMAppService.mainApp
        do {
            if service.status == .enabled { try service.unregister() } else { try service.register() }
        } catch {
            let alert = NSAlert()
            alert.messageText = "Couldn't change Launch at Login"
            alert.informativeText = "\(error.localizedDescription)\n\nMove Notchling.app to /Applications and try again."
            alert.runModal()
        }
    }
}
