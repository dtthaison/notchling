import Foundation
import Security

/// The vault's values: one Keychain item (service `Notchling Secret Vault`, account `values`, JSON of
/// name → value), written through the Security framework so its access list trusts only the Notchling app.
/// Anything else that asks for it, `security find-generic-password` included, gets macOS's own prompt
/// instead of the value. Unlike the account logins (which trust `/usr/bin/security` so a rebuilt app can
/// still read them), losing access on a rebuild is wanted here: macOS asks once ("Always Allow") and the
/// app keeps the values in memory until it quits. Only the app process uses this; the command line asks
/// the app through `VaultServer`.
final class VaultStore {
    static let shared = VaultStore()

    static let service = "Notchling Secret Vault"
    static let account = "values"
    /// Where the first version kept one item per secret, trusting `/usr/bin/security`. Moved, then deleted.
    static let legacyService = "Notchling Vault"

    private let lock = NSLock()
    private var cache: [String: VaultValue]?

    /// Where the list and the values live. The defaults are the real index file and the Keychain item; tests pass
    /// their own, so they never touch either.
    struct Backend {
        /// Throws when the index exists but can't be read, so a write-back never replaces it with an empty list.
        var readIndex: () throws -> [VaultEntry] = { try SecretVault.readIndex() }
        var writeIndex: ([VaultEntry]) throws -> Void = { try SecretVault.write($0) }
        /// nil: the Keychain item.
        var loadValues: (() throws -> [String: VaultValue])?
        var writeValues: (([String: VaultValue]) throws -> Void)?
        var center: NotificationCenter = .default
    }

    private let backend: Backend

    init(backend: Backend = Backend()) { self.backend = backend }

    func value(for entry: VaultEntry) throws -> VaultValue? {
        lock.lock(); defer { lock.unlock() }
        return try load()[entry.id]
    }

    /// Saves an entry; a nil value keeps the stored one (editing the note, URL or variables only).
    /// Renaming moves the stored value to the new name.
    func save(_ entry: VaultEntry, value: VaultValue?, replacing old: VaultEntry? = nil) throws {
        lock.lock(); defer { lock.unlock() }
        var list = try backend.readIndex()
        if let problem = SecretVault.problem(saving: entry, in: list, replacing: old) { throw AccountError(problem) }
        var values = try load()
        var stored = value
        // a rename whose index write failed left the value under the new id: finding it there makes a retry finish
        if stored == nil, let old { stored = values[old.id] ?? values[entry.id] }
        guard let stored, !stored.secret.isEmpty else {
            throw AccountError(entry.kind == .login ? "Enter the password." : "Enter the token.")
        }
        let before = values[old?.id ?? entry.id] ?? values[entry.id]
        if let old { values[old.id] = nil }
        values[entry.id] = stored
        try write(values)
        list.removeAll { $0.id == entry.id || $0.id == old?.id }
        var saved = entry
        saved.updated = Date()
        list.append(saved)
        try backend.writeIndex(list)
        Self.announce(name: entry.name, before: before, after: stored, center: backend.center)
    }

    /// Tells the app a value changed: on the main queue, with the name only, never the value. Machine management
    /// listens and offers to update machines; nothing is sent from here. Equal values post nothing.
    static func announce(name: String, before: VaultValue?, after: VaultValue, center: NotificationCenter = .default) {
        guard changed(old: before, new: after) else { return }
        DispatchQueue.main.async {
            center.post(name: .vaultValueChanged, object: nil, userInfo: ["name": name])
        }
    }

    /// Runs `body` with the store's lock held, so a write to the index outside `save` can't interleave with one.
    func locked<T>(_ body: () throws -> T) rethrows -> T {
        lock.lock(); defer { lock.unlock() }
        return try body()
    }

    /// Renames an entry and keeps its value: the value never leaves the store. Call it only on the user's click.
    /// If the index write fails after the Keychain write, the value sits under the new name while the index still
    /// lists the old one (`save` has always worked this way); the caller should say so and let the user try again.
    func rename(_ entry: VaultEntry, to name: String) throws {
        try save(entry.renamed(to: name), value: nil, replacing: entry)
    }

    /// Whether a save puts a different value in place of the stored one (a new entry counts as different).
    static func changed(old: VaultValue?, new: VaultValue) -> Bool { old != new }

    func remove(_ entry: VaultEntry) throws {
        lock.lock(); defer { lock.unlock() }
        let list = try backend.readIndex()
        var values = try load()
        values[entry.id] = nil
        try write(values)
        try backend.writeIndex(list.filter { $0.id != entry.id })
    }

    /// Turns "ask before each use" off for these entries (the approval's Always Allow).
    func stopAsking(_ ids: Set<String>) {
        lock.lock(); defer { lock.unlock() }
        guard let index = try? backend.readIndex() else { return }
        let list = index.map { e -> VaultEntry in
            var e = e
            if ids.contains(e.id) { e.ask = false }
            return e
        }
        try? backend.writeIndex(list)
    }

    // MARK: Keychain

    private var query: [String: Any] {
        [kSecClass as String: kSecClassGenericPassword,
         kSecAttrService as String: Self.service,
         kSecAttrAccount as String: Self.account]
    }

    /// Call with the lock held.
    private func load() throws -> [String: VaultValue] {
        if let loadValues = backend.loadValues { return try loadValues() }
        if let cache { return cache }
        var q = query
        q[kSecReturnData as String] = true
        q[kSecMatchLimit as String] = kSecMatchLimitOne
        var result: CFTypeRef?
        let status = SecItemCopyMatching(q as CFDictionary, &result)
        switch status {
        case errSecSuccess:
            guard let data = result as? Data,
                  let values = try? JSONDecoder().decode([String: VaultValue].self, from: data) else {
                throw AccountError("The vault's Keychain item can't be read.")
            }
            cache = values
            return values
        case errSecItemNotFound:
            let moved = migrateLegacy()
            if !moved.isEmpty {
                try write(moved)
                for id in moved.keys { Keychain.delete(service: Self.legacyService, account: id) }
            }
            cache = moved
            return moved
        default:
            throw Self.error(status)
        }
    }

    /// Call with the lock held.
    private func write(_ values: [String: VaultValue]) throws {
        if let writeValues = backend.writeValues { return try writeValues(values) }
        let data = try JSONEncoder().encode(values)
        var status = SecItemUpdate(query as CFDictionary, [kSecValueData as String: data] as CFDictionary)
        if status == errSecItemNotFound {
            var add = query
            add[kSecValueData as String] = data
            add[kSecAttrLabel as String] = Self.service
            add[kSecAttrDescription as String] = "Notchling secrets for agents"
            status = SecItemAdd(add as CFDictionary, nil)
        }
        guard status == errSecSuccess else { throw Self.error(status) }
        cache = values
    }

    /// The first version's items, read through `/usr/bin/security` (they trust it), then deleted once the
    /// new item holds them.
    private func migrateLegacy() -> [String: VaultValue] {
        var values: [String: VaultValue] = [:]
        for entry in SecretVault.all() {
            guard let text = Keychain.read(service: Self.legacyService, account: entry.id),
                  let data = Data(base64Encoded: text),
                  let value = try? JSONDecoder().decode(VaultValue.self, from: data) else { continue }
            values[entry.id] = value
        }
        return values
    }

    private static func error(_ status: OSStatus) -> AccountError {
        switch status {
        case errSecUserCanceled, errSecAuthFailed, errSecInteractionNotAllowed:
            return AccountError("Keychain access was denied. Notchling needs it to read the vault; allow it when macOS asks.")
        default:
            let text = SecCopyErrorMessageString(status, nil) as String? ?? "error \(status)"
            return AccountError("Keychain: \(text)")
        }
    }
}
