import Foundation

/// Writes files that hold logins or tokens. `Data.write(.atomic)` creates its temporary file with the
/// umask's mode (usually 0644) and renames it into place, so the secret is readable by others until a
/// later chmod. Here the temporary file is created 0600, then renamed over the target.
enum PrivateFile {
    static func write(_ data: Data, to url: URL) throws {
        let dir = url.deletingLastPathComponent()
        let temp = dir.appendingPathComponent(".\(url.lastPathComponent).\(UUID().uuidString).tmp")
        let fd = open(temp.path, O_WRONLY | O_CREAT | O_EXCL, 0o600)
        guard fd >= 0 else { throw POSIXError(POSIXErrorCode(rawValue: errno) ?? .EIO) }
        var failure: Int32 = 0
        data.withUnsafeBytes { raw in
            var offset = 0
            while offset < raw.count {
                let n = Darwin.write(fd, raw.baseAddress! + offset, raw.count - offset)
                if n < 0 { if errno == EINTR { continue }; failure = errno; return }
                offset += n
            }
        }
        if failure == 0, fsync(fd) != 0 { failure = errno }
        close(fd)
        if failure == 0, rename(temp.path, url.path) != 0 { failure = errno }
        if failure != 0 {
            unlink(temp.path)
            throw POSIXError(POSIXErrorCode(rawValue: failure) ?? .EIO)
        }
    }
}
