import ConsoleCore
import Foundation
import Network
import Security

/// The TLS identity the bridge presents: a self-signed EC P-256 certificate made once with /usr/bin/openssl,
/// kept as a PKCS#12 with a random passphrase in ~/Library/Application Support/AgentConsole/bridge/.
/// The phone pins the certificate's SHA-256 fingerprint (from the QR) instead of trusting a CA.
struct BridgeIdentity {
    let identity: sec_identity_t
    /// SHA-256 of the DER certificate, lowercase hex.
    let fingerprint: String

    enum Failure: Error, LocalizedError {
        case openssl(String)
        case importFailed(OSStatus)
        case noIdentity

        var errorDescription: String? {
            switch self {
            case .openssl(let why): return "Không tạo được chứng chỉ: \(why)"
            case .importFailed(let status): return "Không nạp được chứng chỉ (OSStatus \(status))."
            case .noIdentity: return "File chứng chỉ không có identity."
            }
        }
    }

    static var defaultDirectory: URL {
        FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Library/Application Support/AgentConsole/bridge")
    }

    private static func file(_ name: String, in directory: URL) -> URL { directory.appendingPathComponent(name) }

    static func exists(in directory: URL = defaultDirectory) -> Bool {
        FileManager.default.fileExists(atPath: file("identity.p12", in: directory).path)
    }

    /// Loads the identity, creating it on first use.
    static func loadOrCreate(in directory: URL = defaultDirectory) throws -> BridgeIdentity {
        if !exists(in: directory) { try generate(in: directory) }
        return try load(in: directory)
    }

    /// Deletes the certificate, so the next load makes a new one. Every paired phone stops matching.
    static func erase(in directory: URL = defaultDirectory) {
        for name in ["identity.p12", "identity.pass", "cert.pem", "key.pem"] {
            do { try FileManager.default.removeItem(at: file(name, in: directory)) } catch {
                if (error as NSError).code != NSFileNoSuchFileError { ConsoleLog.error("BridgeIdentity.erase", name, "\(error)") }
            }
        }
    }

    private static func generate(in directory: URL) throws {
        let manager = FileManager.default
        try manager.createDirectory(at: directory, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
        try manager.setAttributes([.posixPermissions: 0o700], ofItemAtPath: directory.path)

        let key = file("key.pem", in: directory), cert = file("cert.pem", in: directory)
        let p12 = file("identity.p12", in: directory), passFile = file("identity.pass", in: directory)
        // Create each file at 0600 first, so the key is never readable by others, not even for a moment.
        let passphrase = BridgeCrypto.randomToken(bytes: 24)
        for url in [key, cert, p12] {
            guard manager.createFile(atPath: url.path, contents: nil, attributes: [.posixPermissions: 0o600]) else {
                throw Failure.openssl("không tạo được \(url.lastPathComponent)")
            }
        }
        guard manager.createFile(atPath: passFile.path, contents: Data(passphrase.utf8), attributes: [.posixPermissions: 0o600]) else {
            throw Failure.openssl("không ghi được passphrase")
        }

        let host = ProcessInfo.processInfo.hostName
        try openssl(["ecparam", "-name", "prime256v1", "-genkey", "-noout", "-out", key.path])
        try openssl(["req", "-new", "-x509", "-key", key.path, "-out", cert.path, "-days", "3650", "-sha256",
                     "-subj", "/CN=Agent Console (\(host))"])
        // 3DES/SHA-1 is what Security.framework reads on every macOS we support; OpenSSL's newer default does not.
        try openssl(["pkcs12", "-export", "-inkey", key.path, "-in", cert.path, "-out", p12.path,
                     "-passout", "file:\(passFile.path)", "-certpbe", "PBE-SHA1-3DES", "-keypbe", "PBE-SHA1-3DES", "-macalg", "sha1"])
        // The key lives inside the PKCS#12 from here on.
        do { try manager.removeItem(at: key) } catch { ConsoleLog.error("BridgeIdentity.generate", key.path, "\(error)") }
        for url in [cert, p12, passFile] { try manager.setAttributes([.posixPermissions: 0o600], ofItemAtPath: url.path) }
    }

    private static func openssl(_ arguments: [String]) throws {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/openssl")
        process.arguments = arguments
        let errors = Pipe()
        process.standardError = errors
        process.standardOutput = FileHandle.nullDevice
        do { try process.run() } catch { throw Failure.openssl("\(error.localizedDescription)") }
        let message = String(decoding: errors.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
        process.waitUntilExit()
        guard process.terminationStatus == 0 else {
            throw Failure.openssl("openssl \(arguments.first ?? "") thoát \(process.terminationStatus): \(message.prefix(300))")
        }
    }

    private static func load(in directory: URL) throws -> BridgeIdentity {
        let data = try Data(contentsOf: file("identity.p12", in: directory))
        let passphrase = try String(contentsOf: file("identity.pass", in: directory), encoding: .utf8)
        var options: [String: Any] = [kSecImportExportPassphrase as String: passphrase]
        // Keeps the key out of the user's keychain (macOS 15 and later).
        if #available(macOS 15.0, *) { options[kSecImportToMemoryOnly as String] = true }

        var items: CFArray?
        let status = SecPKCS12Import(data as CFData, options as CFDictionary, &items)
        guard status == errSecSuccess else { throw Failure.importFailed(status) }
        guard let entry = (items as? [[String: Any]])?.first,
              let raw = entry[kSecImportItemIdentity as String] else { throw Failure.noIdentity }
        let secIdentity = raw as! SecIdentity
        var certificate: SecCertificate?
        SecIdentityCopyCertificate(secIdentity, &certificate)
        guard let certificate, let identity = sec_identity_create(secIdentity) else { throw Failure.noIdentity }
        let der = SecCertificateCopyData(certificate) as Data
        return BridgeIdentity(identity: identity, fingerprint: BridgeCrypto.sha256Hex(der))
    }
}
