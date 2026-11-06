import AppKit
import ConsoleCore
import ServiceManagement

final class AppDelegate: NSObject, NSApplicationDelegate, NSMenuDelegate {
    private let awake = AwakeController()
    private let resumer = WakeResumer()
    private let monitor = AgentMonitor()
    private let notch = NotchController()
    private let accounts = AccountStore()
    private let antigravity = AntigravityStore()
    private let claudeDesktop = ClaudeDesktopStore()
    private let codex = CodexStore()
    private let cost = CostTracker()
    private let settings = SettingsModel()
    private lazy var limits = LimitWatcher(accounts: accounts) { [weak self] in self?.announceLimit($0) }
    private lazy var lanesMenu = LanesMenu(accounts: accounts)
    /// Every account provider, in the order the Accounts window tabs and the menu sections show them.
    /// A new provider is registered here and nowhere else.
    private lazy var providers: ProviderRegistry = {
        let registry = ProviderRegistry()
        registry.register(ClaudeCodeProvider(store: accounts))
        registry.register(ClaudeDesktopProvider(store: claudeDesktop, claude: accounts))
        registry.register(CodexProvider(store: codex))
        registry.register(AntigravityProvider(store: antigravity))
        return registry
    }()
    private lazy var accountsWindow = AccountsWindowController(registry: providers, accounts: accounts, cost: cost, settings: settings)
    private lazy var pagesWindow = PagesWindowController(accounts: accounts)
    private lazy var search: SearchPanelController = {
        let search = SearchPanelController()
        search.onOpen = { [weak self] folder in self?.chatWindow.showPage(folder: folder) }
        search.onOpenSession = { [weak self] session in self?.chatWindow.showSession(session) }
        return search
    }()
    /// ⌘⇧P was free when Notchling started (another app may hold it).
    private var searchShortcut = false
    private lazy var chatWindow: ChatWindowController = {
        let chat = ChatWindowController(notch: notch.model, accounts: accounts, codex: codex, antigravity: antigravity)
        // An artifact link in a conversation opens Notchling's saved copy, readable from any account.
        chat.onOpenArtifact = { [weak self] folder in
            self?.pagesWindow.show(select: folder)
            return self != nil
        }
        return chat
    }()
    private var statusItem: NSStatusItem!
    private var agents: [Agent] = []
    private var lastGroups: [String: AgentGroup]?
    private var timer: Timer?
    private var seenRequests = Set<String>()
    private var usageTimer: Timer?
    /// Rows in the open menu, updated in place when limits arrive.
    private var accountRows: [String: AccountRowView] = [:]

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
        PointerCursors.install()
        DockPresence.start()
        searchShortcut = search.start()
        statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.squareLength)
        statusItem.button?.imagePosition = .imageLeading
        let menu = NSMenu()
        menu.delegate = self
        statusItem.menu = menu

        HookBridge.beat()
        // The vault's socket for `Notchling --secret`, and its skill (rewritten if the app moved).
        VaultServer.shared.start()
        DispatchQueue.global(qos: .utility).async { VaultSkill.update(); DelegateSkill.update() }
        // The phone bridge, the session host connection and the sessions a phone can reach.
        Task { @MainActor [weak self] in
            PhoneService.shared.limitsSource = { [weak self] in self?.phoneLimits() ?? [] }
            PhoneService.shared.start()
        }
        // First launch with this version: turn on answering prompts from the notch.
        if UserDefaults.standard.object(forKey: "answerFromNotch") == nil {
            setAnswerFromNotch(true)
        } else if answerFromNotch, !HookBridge.isInstalled {
            setAnswerFromNotch(true)
        }

        // Same for keeping local copies of published pages; its hook is installed on its own. Reinstalled
        // every launch (a no-op when nothing changed) so older installs get the read hook and the skill.
        if keepPages { setKeepPages(true) }

        refresh()
        if showNotch { notch.show() }
        timer = Timer.scheduledTimer(withTimeInterval: 3, repeats: true) { [weak self] _ in self?.refresh() }

        settings.apply = { [weak self] in self?.applySetting($0) }
        settings.reload = { [weak self] in self?.reloadSettings() }
        for provider in providers.providers {
            provider.changed = provider.id == .claudeCode
                ? { [weak self] in self?.updateUsageViews() }
                : { [weak self] in self?.updateButton() }
        }
        cost.changed = { [weak self] in self?.updateUsageViews() }
        providers.syncAll()
        cost.refresh()
        usageTimer = Timer.scheduledTimer(withTimeInterval: 60, repeats: true) { [weak self] _ in
            self?.providers.syncAll()
            self?.cost.refresh()
        }
        // An install relaunches Notchling: bring Chat back if it was open, once sessions are listed.
        DispatchQueue.main.asyncAfter(deadline: .now() + 1.5) { [weak self] in self?.chatWindow.restore() }
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

    private var keepPages: Bool {
        UserDefaults.standard.object(forKey: "keepPages") as? Bool ?? true
    }

    private func setKeepPages(_ on: Bool) {
        do {
            try HookBridge.setPagesInstalled(on)
            UserDefaults.standard.set(on, forKey: "keepPages")
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
        for key in ["mode", "active", "keepDisplayOn", "continueAfterSleep", "hideNotch", "answerFromNotch", "keepPages", "showPet", "replyFromNotch", "showLastReply", "askedAccessibility", "notchWidth", "dismissed", "hiddenAgents", "accounts", "untrackedCostAccount", "antigravityAccounts", "claudeDesktopAccounts", "codexAccounts", "limitMode", "limitSwitchAt"] {
            if let value = old[key], defaults.object(forKey: key) == nil { defaults.set(value, forKey: key) }
        }
        defaults.set(true, forKey: "migratedFromAwake")
    }

    /// The Dock icon (shown only while a window is open) brings that window back, even when minimized.
    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
        let windows = NSApp.windows.filter { !($0 is NSPanel) && $0.styleMask.contains(.titled) }
        if let window = windows.first(where: \.isVisible) ?? windows.first(where: \.isMiniaturized) {
            if window.isMiniaturized { window.deminiaturize(nil) }
            window.makeKeyAndOrderFront(nil)
        } else {
            chatWindow.show()
        }
        return false
    }

    /// `agentconsole://open[?phones=1]`: bring Chat forward; with `phones=1`, paired phones are told to open too.
    func application(_ application: NSApplication, open urls: [URL]) {
        for url in urls {
            guard case .open(let phones)? = AppURL.parse(url) else {
                NSLog("application(open:): not an agentconsole://open URL, ignored: %@", url.absoluteString)
                continue
            }
            chatWindow.show()
            if phones { Task { @MainActor in PhoneService.shared.notifyPhonesToOpen() } }
        }
    }

    func applicationWillTerminate(_ notification: Notification) {
        cost.saveNow()
        awake.stop()
        AntigravityRunner.shared.closeAll()
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
        notch.model.onOpenChat = { [weak self] in self?.chatWindow.show(select: $0) }
        notch.model.onOpenTerminal = { [weak self] in self?.chatWindow.showTerminal(select: $0, command: $1) }
        if let request = fresh.first, let agent = agents.first(where: { $0.id == request.sessionID }), notch.isVisible {
            NSSound(named: "Tink")?.play()
            notch.model.peek(agent)
        }
        notch.model.refreshSnapshots { [weak self] old, new in self?.peekTransitions(old: old, new: new) }
        awake.apply(agentsWorking: agents.contains { $0.group == .working })
        resumer.check(agents)
        limits.check(agents)
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
        let resumeItem = action("Continue Agents After Sleep", #selector(toggleResume))
        resumeItem.state = resumer.enabled ? .on : .off
        resumeItem.toolTip = "When the Mac wakes, background agents whose reply was cut off by sleep are told to continue."
        menu.addItem(resumeItem)

        menu.addItem(.separator())
        let chatItem = action("Chat…", #selector(openChat))
        chatItem.toolTip = "Read every Claude Code session and reply to background ones."
        menu.addItem(chatItem)
        let searchItem = action("Search Artifacts…", #selector(openSearch))
        if searchShortcut {
            searchItem.keyEquivalent = "p"
            searchItem.keyEquivalentModifierMask = [.command, .shift]
        }
        searchItem.toolTip = searchShortcut ? "Find a saved Artifact from anywhere with ⌘⇧P."
            : "Find a saved Artifact. ⌘⇧P is taken by another app."
        menu.addItem(searchItem)

        menu.addItem(.separator())
        menu.addItem(header("Notch"))
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
        let lastReplyItem = action("Show Last Reply When Peeking", #selector(toggleLastReply))
        lastReplyItem.state = notch.model.showLastReply ? .on : .off
        lastReplyItem.toolTip = "Off, a peek only says which agent finished or wants you."
        menu.addItem(lastReplyItem)
        let replyItem = action("Reply to Background Agents from the Notch", #selector(toggleReply))
        replyItem.state = notch.model.replyFromNotch ? .on : .off
        replyItem.toolTip = "Off, the notch opens the agent in a terminal instead of showing a reply box."
        menu.addItem(replyItem)
        let answerItem = action("Answer Prompts from the Notch", #selector(toggleAnswer))
        answerItem.state = answerFromNotch && HookBridge.isInstalled ? .on : .off
        answerItem.toolTip = "Adds a hook to Claude Code so questions and permission prompts can be answered here."
        menu.addItem(answerItem)
        let pagesItem = action("Keep Local Copies of Artifacts", #selector(toggleKeepPages))
        pagesItem.state = keepPages && HookBridge.pagesInstalled ? .on : .off
        pagesItem.toolTip = "Adds a hook to Claude Code that saves each published Artifact page on this Mac, so it stays readable after switching accounts."
        menu.addItem(pagesItem)
        menu.addItem(localPagesItem())
        menu.addItem(.separator())
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
        menu.addItem(lanesMenu.item())
        // Opens the cost window; the breakdown lives there, not in a submenu.
        menu.addItem(action(costTitle, #selector(showCost)))
        menu.addItem(limitItem())

        for provider in providers.providers where provider.id != .claudeCode { addProviderItems(provider, to: menu) }

        menu.addItem(.separator())
        menu.addItem(action("Manage Accounts…", #selector(manageAccounts)))
        menu.addItem(action("Open Chat and Phones", #selector(openChatAndPhones)))
        let settingsItem = action("Settings…", #selector(showSettings))
        settingsItem.keyEquivalent = ","
        menu.addItem(settingsItem)
    }

    /// "When the Account in Use Nears Its Limit": what to do, and the percentage to switch at.
    private func limitItem() -> NSMenuItem {
        let sub = NSMenu()
        for mode in LimitMode.allCases {
            let item = action(mode.title, #selector(setLimitMode(_:)))
            item.representedObject = mode.rawValue
            item.state = limits.mode == mode ? .on : .off
            sub.addItem(item)
        }
        sub.addItem(.separator())
        sub.addItem(header("Switch At"))
        var points = [90, 95, 99]
        if !points.contains(limits.switchAt) { points.append(limits.switchAt) }
        for point in points {
            let custom = ![90, 95, 99].contains(point)
            let item = action(custom ? "Custom: \(point)%" : "\(point)%", #selector(setLimitSwitchAt(_:)))
            item.representedObject = point
            item.state = limits.switchAt == point ? .on : .off
            item.isEnabled = limits.mode == .notifyAndSwitch
            sub.addItem(item)
        }
        let item = NSMenuItem(title: "When the Account in Use Nears Its Limit", action: nil, keyEquivalent: "")
        item.submenu = sub
        item.toolTip = "Notify at 90% and 100% of the 5-hour or weekly limit; or also switch to the saved account with the most room."
        return item
    }

    @objc private func setLimitMode(_ sender: NSMenuItem) {
        if let raw = sender.representedObject as? String, let mode = LimitMode(rawValue: raw) { limits.mode = mode }
    }

    @objc private func setLimitSwitchAt(_ sender: NSMenuItem) {
        if let point = sender.representedObject as? Int { limits.switchAt = point }
    }

    /// A usage-limit message: sound and notch peek like an agent's reply, plus a macOS notification.
    private func announceLimit(_ text: String) {
        NSSound(named: "Tink")?.play()
        if notch.isVisible, let agent = agents.first { notch.model.peekNotice(text, using: agent) }
        UserNotifier.post("Notchling", text)
        chatWindow.noteLimitSwitch(text)
    }

    /// One provider's menu section: the account in use (with its limit bars, if it has them), then a
    /// Switch Account submenu. The Claude Code section above is its own.
    private func addProviderItems(_ provider: any AccountProvider, to menu: NSMenu) {
        guard provider.isAvailable else { return }
        let style = provider.menuStyle
        if style.syncsOnOpen { provider.sync(force: false) }
        let state = provider.state
        menu.addItem(.separator())
        menu.addItem(header(provider.title))
        if state.adding && style.waitsInMenu {
            menu.addItem(disabled("Waiting for you to sign in…"))
            let cancel = action("Cancel Add Account", #selector(cancelProviderAdd(_:)))
            cancel.representedObject = ProviderMenuTarget(provider: provider.id)
            menu.addItem(cancel)
            return
        }
        if let name = state.activeName {
            let opens = ProviderMenuTarget(provider: style.opens ?? provider.id)
            let item = action(name, #selector(manageProvider(_:)))
            item.representedObject = opens
            if style.showsLimits, let account = state.active {
                let row = GroupedUsageRowView(email: account.email, plan: account.plan ?? style.planFallback,
                                              usage: provider.usageReport(for: account.id), placeholder: style.limitsPlaceholder)
                row.onClick = { [weak self] in self?.accountsWindow.show(.accounts, provider: opens.provider) }
                item.view = row
            } else {
                item.state = .on
            }
            menu.addItem(item)
        } else {
            menu.addItem(disabled(style.notSignedIn))
        }
        let switchItem = NSMenuItem(title: "Switch Account", action: nil, keyEquivalent: "")
        switchItem.submenu = providerSwitchMenu(provider, state: state)
        menu.addItem(switchItem)
    }

    private func providerSwitchMenu(_ provider: any AccountProvider, state: ProviderState) -> NSMenu {
        let submenu = NSMenu()
        submenu.autoenablesItems = false
        for account in state.accounts {
            let item = action(account.email, #selector(switchProviderItem(_:)))
            item.representedObject = ProviderMenuTarget(provider: provider.id, account: account.id)
            item.state = account.id == state.activeID ? .on : .off
            if let plan = account.plan { item.attributedTitle = accountTitle(account.email, detail: [plan]) }
            submenu.addItem(item)
        }
        if !state.accounts.isEmpty { submenu.addItem(.separator()) }
        let add = action("Add Account…", provider.capabilities.contains(.addFromMenu)
                         ? #selector(addProviderAccount(_:)) : #selector(manageProvider(_:)))
        add.representedObject = ProviderMenuTarget(provider: provider.id)
        submenu.addItem(add)
        return submenu
    }

    @objc private func cancelProviderAdd(_ sender: NSMenuItem) {
        guard let target = sender.representedObject as? ProviderMenuTarget else { return }
        providers[target.provider]?.cancelAdd()
    }

    @objc private func manageProvider(_ sender: NSMenuItem) {
        guard let target = sender.representedObject as? ProviderMenuTarget else { return }
        accountsWindow.show(.accounts, provider: target.provider)
    }

    @objc private func switchProviderItem(_ sender: NSMenuItem) {
        guard let target = sender.representedObject as? ProviderMenuTarget, let id = target.account,
              let provider = providers[target.provider], id != provider.state.activeID else { return }
        if let ask = provider.switchConfirmation(to: id), !confirm(ask) { return }
        Task { @MainActor in
            do { try await provider.switchTo(id) } catch { self.showError(provider.switchFailureTitle, error) }
        }
    }

    /// Add Account… for a provider whose flow the menu starts itself (it may restart an app first).
    @objc private func addProviderAccount(_ sender: NSMenuItem) {
        guard let target = sender.representedObject as? ProviderMenuTarget, let provider = providers[target.provider] else { return }
        if let ask = provider.addConfirmation(), !confirm(ask) { return }
        provider.beginAdd { [weak self] error in
            if let error { self?.showError("Couldn't open \(provider.title) to add an account", error) }
        }
    }

    private func confirm(_ ask: ProviderConfirmation) -> Bool {
        let alert = NSAlert()
        alert.messageText = ask.title
        alert.informativeText = ask.message
        alert.addButton(withTitle: ask.button)
        alert.addButton(withTitle: "Cancel")
        NSApp.activate(ignoringOtherApps: true)
        return alert.runModal() == .alertFirstButtonReturn
    }

    private func showError(_ title: String, _ error: Error) {
        let alert = NSAlert()
        alert.messageText = title
        alert.informativeText = error.localizedDescription
        NSApp.activate(ignoringOtherApps: true)
        alert.runModal()
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

    /// Limits or cost arrived while the menu may be open: redraw what's on screen.
    private func updateUsageViews() {
        for (id, row) in accountRows {
            guard let account = accounts.accounts.first(where: { $0.id == id }) else { continue }
            row.account = account
            row.isActive = id == accounts.activeID
            row.usage = accounts.usage[id]
            row.relayout()
        }
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

    /// Opens Chat and, with the phone bridge on, tells paired phones to open too.
    @objc private func openChatAndPhones() {
        chatWindow.show()
        Task { @MainActor in PhoneService.shared.notifyPhonesToOpen() }
    }

    @objc private func showSettings() {
        accountsWindow.show(.settings)
    }

    // MARK: - Settings tab

    /// Copies the real settings into the Settings tab, touching only what changed.
    private func reloadSettings() {
        let login = SMAppService.mainApp.status == .enabled
        let answer = answerFromNotch && HookBridge.isInstalled
        let pages = keepPages && HookBridge.pagesInstalled
        settings.load { s in
            if s.awakeMode != awake.mode { s.awakeMode = awake.mode }
            if s.keepDisplayOn != awake.keepDisplayOn { s.keepDisplayOn = awake.keepDisplayOn }
            if s.continueAfterSleep != resumer.enabled { s.continueAfterSleep = resumer.enabled }
            if s.showNotch != showNotch { s.showNotch = showNotch }
            if s.notchWidth != notch.model.width { s.notchWidth = notch.model.width }
            if s.animatePet != notch.model.showPet { s.animatePet = notch.model.showPet }
            if s.showLastReply != notch.model.showLastReply { s.showLastReply = notch.model.showLastReply }
            if s.replyFromNotch != notch.model.replyFromNotch { s.replyFromNotch = notch.model.replyFromNotch }
            if s.answerFromNotch != answer { s.answerFromNotch = answer }
            if s.keepPages != pages { s.keepPages = pages }
            if s.launchAtLogin != login { s.launchAtLogin = login }
            if s.limitMode != limits.mode { s.limitMode = limits.mode }
            if s.limitSwitchAt != limits.switchAt { s.limitSwitchAt = limits.switchAt }
        }
    }

    private func applySetting(_ change: SettingsModel.Change) {
        switch change {
        case .awakeMode(let mode):
            awake.mode = mode
            refresh()
        case .keepDisplayOn(let on):
            if awake.keepDisplayOn != on { toggleDisplay() }
        case .continueAfterSleep(let on):
            resumer.enabled = on
        case .showNotch(let on):
            if showNotch != on { toggleNotch() }
        case .notchWidth(let width):
            notch.model.width = width
        case .animatePet(let on):
            notch.model.showPet = on
        case .showLastReply(let on):
            notch.model.showLastReply = on
        case .replyFromNotch(let on):
            notch.model.replyFromNotch = on
        case .answerFromNotch(let on):
            setAnswerFromNotch(on)
        case .keepPages(let on):
            setKeepPages(on)
        case .limitMode(let mode):
            limits.mode = mode
        case .limitSwitchAt(let point):
            limits.switchAt = point
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

    @objc private func toggleReply() {
        notch.model.replyFromNotch.toggle()
    }

    @objc private func toggleLastReply() {
        notch.model.showLastReply.toggle()
    }

    @objc private func toggleAnswer() {
        setAnswerFromNotch(!(answerFromNotch && HookBridge.isInstalled))
    }

    @objc private func toggleKeepPages() {
        setKeepPages(!(keepPages && HookBridge.pagesInstalled))
    }

    /// The newest saved pages, plus the window with all of them and the folder.
    private func localPagesItem() -> NSMenuItem {
        let item = NSMenuItem(title: "Artifacts", action: nil, keyEquivalent: "")
        let sub = NSMenu()
        let pages = PageLibrary.entries()
        if pages.isEmpty {
            sub.addItem(disabled("None Saved Yet"))
        }
        for page in pages.prefix(12) {
            let row = action(String(page.title.prefix(60)), #selector(openPage(_:)))
            row.representedObject = page.folder
            row.toolTip = page.account.isEmpty ? page.project : "\(page.account)\n\(page.project)"
            sub.addItem(row)
        }
        sub.addItem(.separator())
        sub.addItem(action("All Artifacts…", #selector(openAllPages)))
        let show = action("Show in Finder", #selector(showPagesInFinder))
        show.isEnabled = !pages.isEmpty
        sub.addItem(show)
        item.submenu = sub
        return item
    }

    /// A page from the menu opens beside the chat, like one from search.
    @objc private func openPage(_ sender: NSMenuItem) {
        guard let folder = sender.representedObject as? String else { return }
        chatWindow.showPage(folder: folder)
    }

    /// The active account's 5-hour and weekly limits, for the phone's chat header.
    private func phoneLimits() -> [LimitInfo] {
        guard let id = accounts.activeID, let usage = accounts.usage[id] else { return [] }
        return [usage.session.map { LimitInfo(label: "5h", percent: $0.percent, resetsAt: $0.resetsAt) },
                usage.week.map { LimitInfo(label: "Week", percent: $0.percent, resetsAt: $0.resetsAt) }].compactMap { $0 }
    }

    @objc private func openChat() {
        chatWindow.show()
    }

    @objc private func openSearch() {
        search.show()
    }

    @objc private func openAllPages() {
        pagesWindow.show()
    }

    @objc private func showPagesInFinder() {
        NSWorkspace.shared.activateFileViewerSelecting([PageLibrary.indexURL])
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
            // A session moved to another account left a stopped copy behind; remove that too, or it shows
            // up again in place of the one removed. Only the current copy's result counts.
            let dirs = AgentMonitor.jobDirs(forJob: id)
            var failed = false
            var output = ""
            for (index, dir) in (dirs.isEmpty ? [nil] : dirs).enumerated() {
                let result = Self.removeJob(id, configDir: dir)
                if index == 0 { (failed, output) = result }
            }
            DispatchQueue.main.async {
                self.refresh()
                if !failed { NotificationCenter.default.post(name: .sessionRemoved, object: agent.id) }
                guard failed else { return }
                let note = NSAlert()
                note.messageText = "Couldn't remove “\(agent.name)”"
                note.informativeText = output.trimmingCharacters(in: .whitespacesAndNewlines)
                NSApp.activate(ignoringOtherApps: true)
                note.runModal()
            }
        }
    }

    /// `claude rm <id>` in one config folder; whether it failed, and what it printed.
    private static func removeJob(_ id: String, configDir: String?) -> (failed: Bool, output: String) {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: ReplySender.claudePath)
        process.arguments = ["rm", id]
        process.environment = AgentMonitor.environment(configDir: configDir)
        let pipe = Pipe()
        process.standardOutput = pipe
        process.standardError = pipe
        do {
            try process.run()
        } catch {
            return (true, error.localizedDescription)
        }
        let output = String(data: pipe.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8) ?? ""
        process.waitUntilExit()
        return (process.terminationStatus != 0, output)
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

    @objc private func toggleResume() {
        resumer.enabled.toggle()
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
