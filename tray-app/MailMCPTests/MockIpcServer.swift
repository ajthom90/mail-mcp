import Foundation
import Darwin

/// Single-connection UDS server. Records each line received, replies with the
/// canned response keyed by JSON-RPC method.
///
/// Implemented with raw BSD sockets — `NWListener` over UDS proved unreliable
/// in the test sandbox (NECP errors, intermittent `bind` failures), and even
/// when it bound, the partner `NWConnection.receive` callback wouldn't fire
/// after `cancel()`, hanging tear-down.
final class MockIpcServer {
    let socketURL: URL
    var responses: [String: String] = [:]
    var notifications: [String] = []           // pre-canned notifications to push
    var receivedLines: [String] = []

    private var listenFd: Int32 = -1
    private var clientFd: Int32 = -1
    private var acceptThread: Thread?
    private var readThread: Thread?
    private let lock = NSLock()
    private var running = false

    init() {
        // sockaddr_un.sun_path on Darwin is 104 bytes — keep the path well
        // under that. NSTemporaryDirectory() (/var/folders/.../T/) plus a UUID
        // exceeds the limit, so use /private/tmp.
        let shortId = String(UUID().uuidString.prefix(8))
        let dir = URL(fileURLWithPath: "/private/tmp/mmcp-\(shortId)")
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        socketURL = dir.appendingPathComponent("s")
    }

    func start() throws {
        try? FileManager.default.removeItem(at: socketURL)

        let s = socket(AF_UNIX, SOCK_STREAM, 0)
        guard s >= 0 else {
            throw NSError(
                domain: "MockIpcServer", code: Int(errno),
                userInfo: [NSLocalizedDescriptionKey: String(cString: strerror(errno))]
            )
        }

        var addr = sockaddr_un()
        addr.sun_family = sa_family_t(AF_UNIX)
        let path = socketURL.path
        guard path.utf8.count < 104 else {
            close(s)
            throw NSError(domain: "MockIpcServer", code: -1,
                          userInfo: [NSLocalizedDescriptionKey: "socket path too long"])
        }
        withUnsafeMutablePointer(to: &addr.sun_path) { ptr in
            ptr.withMemoryRebound(to: CChar.self, capacity: 104) { dst in
                _ = path.withCString { src in
                    strncpy(dst, src, 103)
                }
            }
        }

        let bindResult: Int32 = withUnsafePointer(to: &addr) { ap in
            ap.withMemoryRebound(to: sockaddr.self, capacity: 1) { sa in
                Darwin.bind(s, sa, socklen_t(MemoryLayout<sockaddr_un>.size))
            }
        }
        if bindResult < 0 {
            let msg = String(cString: strerror(errno))
            close(s)
            throw NSError(domain: "MockIpcServer", code: Int(errno),
                          userInfo: [NSLocalizedDescriptionKey: "bind failed: \(msg)"])
        }
        if Darwin.listen(s, 1) < 0 {
            let msg = String(cString: strerror(errno))
            close(s)
            throw NSError(domain: "MockIpcServer", code: Int(errno),
                          userInfo: [NSLocalizedDescriptionKey: "listen failed: \(msg)"])
        }

        listenFd = s
        running = true

        // Accept on a thread so the client can connect synchronously.
        let t = Thread { [weak self] in self?.acceptLoop() }
        t.name = "MockIpcServer.accept"
        acceptThread = t
        t.start()
    }

    func stop() {
        lock.lock()
        running = false
        let cfd = clientFd
        let lfd = listenFd
        clientFd = -1
        listenFd = -1
        lock.unlock()
        if cfd >= 0 { close(cfd) }
        if lfd >= 0 { close(lfd) }
        try? FileManager.default.removeItem(at: socketURL.deletingLastPathComponent())
    }

    /// Push a notification frame to the connected client (must be after client connects).
    func push(notification: String) {
        let collapsed = notification
            .replacingOccurrences(of: "\n", with: "")
            .replacingOccurrences(of: "\r", with: "")
        notifications.append(collapsed)
        sendLine(collapsed)
    }

    // MARK: - Internals

    private func acceptLoop() {
        let lfd = listenFd
        guard lfd >= 0 else { return }
        var addr = sockaddr_un()
        var len = socklen_t(MemoryLayout<sockaddr_un>.size)
        let cfd: Int32 = withUnsafeMutablePointer(to: &addr) { ap in
            ap.withMemoryRebound(to: sockaddr.self, capacity: 1) { sa in
                Darwin.accept(lfd, sa, &len)
            }
        }
        guard cfd >= 0 else { return }

        lock.lock()
        guard running else { lock.unlock(); close(cfd); return }
        clientFd = cfd
        lock.unlock()

        // Push any notifications that were queued before connect.
        for n in notifications { sendLine(n) }

        // Spawn a reader thread.
        let r = Thread { [weak self] in self?.readLoop(fd: cfd) }
        r.name = "MockIpcServer.read"
        readThread = r
        r.start()
    }

    private func readLoop(fd: Int32) {
        var buffer = Data()
        let chunkSize = 4096
        let chunk = UnsafeMutablePointer<UInt8>.allocate(capacity: chunkSize)
        defer { chunk.deallocate() }
        while true {
            let n = Darwin.read(fd, chunk, chunkSize)
            if n <= 0 { return }
            buffer.append(chunk, count: n)
            while let nl = buffer.firstIndex(of: 0x0a) {
                let line = Data(buffer[buffer.startIndex..<nl])
                buffer.removeSubrange(buffer.startIndex...nl)
                let s = String(data: line, encoding: .utf8) ?? ""
                receivedLines.append(s)
                respond(to: s)
            }
        }
    }

    private func respond(to line: String) {
        struct Req: Decodable { let id: UInt64?; let method: String? }
        guard let req = try? JSONDecoder().decode(Req.self, from: Data(line.utf8)),
              let method = req.method,
              let template = responses[method] ?? defaultResponse(for: method, id: req.id)
        else { return }
        // Tests use multi-line string literals for readability; strip any embedded
        // newlines so the wire frame stays single-line (newlines are the frame
        // delimiter).
        let collapsed = template
            .replacingOccurrences(of: "\n", with: "")
            .replacingOccurrences(of: "\r", with: "")
        let body = collapsed.replacingOccurrences(of: "$ID", with: String(req.id ?? 0))
        sendLine(body)
    }

    private func defaultResponse(for method: String, id: UInt64?) -> String? {
        if method == "subscribe" {
            return "{\"jsonrpc\":\"2.0\",\"id\":\(id ?? 0),\"result\":{\"subscribed\":[]}}"
        }
        return nil
    }

    private func sendLine(_ line: String) {
        lock.lock()
        let cfd = clientFd
        lock.unlock()
        guard cfd >= 0 else { return }
        var data = Data(line.utf8)
        data.append(0x0a)
        data.withUnsafeBytes { (buf: UnsafeRawBufferPointer) in
            var bytesLeft = data.count
            var ptr = buf.baseAddress!
            while bytesLeft > 0 {
                let w = Darwin.write(cfd, ptr, bytesLeft)
                if w <= 0 { return }
                bytesLeft -= w
                ptr = ptr.advanced(by: w)
            }
        }
    }
}
