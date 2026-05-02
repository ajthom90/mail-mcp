import Foundation
import Darwin

/// JSON-RPC 2.0 client over a Unix domain socket. Newline-delimited frames.
/// Concurrency: actor — every request and notification is funneled through one task.
///
/// Built on BSD sockets (`socket(2)`/`connect(2)`) rather than `NWConnection`
/// because the Network.framework receive callback does not reliably fire after
/// `cancel()` over a UDS, hanging tests during teardown. `close(fd)` produces
/// deterministic EOF on a blocking `read(2)`, which gives us clean shutdown.
public actor IpcClient {
    public enum IpcError: Error, LocalizedError {
        case connectionRefused(String)
        case connectionClosed
        case rpcError(code: Int, message: String)
        case decoding(Error)
        case encoding(Error)
        case timeout

        public var errorDescription: String? {
            switch self {
            case .connectionRefused(let s): return "IPC connection refused: \(s)"
            case .connectionClosed:         return "Daemon closed the IPC connection"
            case .rpcError(let c, let m):   return "Daemon error \(c): \(m)"
            case .decoding(let e):          return "Decoding failed: \(e)"
            case .encoding(let e):          return "Encoding failed: \(e)"
            case .timeout:                  return "IPC request timed out"
            }
        }
    }

    private let socketPath: URL
    private var fd: Int32 = -1
    private var nextId: UInt64 = 1
    private var pending: [UInt64: CheckedContinuation<Data, Error>] = [:]
    /// Frames routed to an `id` whose continuation hasn't been registered yet.
    /// `call` checks this first when it goes to register, eliminating the race
    /// where a fast response arrives before the awaiter is parked.
    private var earlyResponses: [UInt64: Data] = [:]
    private var notificationContinuations: [UUID: AsyncStream<DaemonNotification>.Continuation] = [:]
    private var readerTask: Task<Void, Never>?
    private var isClosed = false

    public init(socketPath: URL) {
        self.socketPath = socketPath
    }

    /// Connect (or no-op if already connected). Throws if the socket is missing.
    public func connect() async throws {
        if fd >= 0 && !isClosed { return }

        let path = socketPath.path
        let pathBytes = Array(path.utf8)
        // sockaddr_un.sun_path is 104 bytes on Darwin; require room for NUL.
        guard pathBytes.count < 104 else {
            throw IpcError.connectionRefused("socket path too long")
        }

        let s = socket(AF_UNIX, SOCK_STREAM, 0)
        guard s >= 0 else {
            throw IpcError.connectionRefused(String(cString: strerror(errno)))
        }

        var addr = sockaddr_un()
        addr.sun_family = sa_family_t(AF_UNIX)
        // Copy path into sun_path. `strncpy` zero-pads, ensuring NUL termination
        // for any path under 103 bytes.
        withUnsafeMutablePointer(to: &addr.sun_path) { ptr in
            ptr.withMemoryRebound(to: CChar.self, capacity: 104) { dst in
                _ = path.withCString { src in
                    strncpy(dst, src, 103)
                }
            }
        }

        let result: Int32 = withUnsafePointer(to: &addr) { ap in
            ap.withMemoryRebound(to: sockaddr.self, capacity: 1) { sa in
                Darwin.connect(s, sa, socklen_t(MemoryLayout<sockaddr_un>.size))
            }
        }
        if result < 0 {
            let err = String(cString: strerror(errno))
            close(s)
            throw IpcError.connectionRefused(err)
        }

        self.fd = s
        self.isClosed = false

        // Spawn a detached reader that does blocking reads. When `disconnect()`
        // closes the fd, the read returns 0/-1 and the loop exits. The reader
        // is `nonisolated` so its blocking `read(2)` does not occupy the actor's
        // executor — `disconnect()` can therefore run while the reader is parked
        // in `read(2)`, close the fd, and unblock the reader via EBADF.
        let myFd = s
        self.readerTask = Task.detached { [weak self] in
            await Self.readLoop(fd: myFd, owner: self)
        }
    }

    public func disconnect() {
        guard !isClosed else { return }
        isClosed = true
        if fd >= 0 {
            close(fd)
            fd = -1
        }
        readerTask?.cancel()
        readerTask = nil
        for (_, c) in pending { c.resume(throwing: IpcError.connectionClosed) }
        pending.removeAll()
        for (_, c) in notificationContinuations { c.finish() }
        notificationContinuations.removeAll()
    }

    /// Send a JSON-RPC request and await the matching response.
    public func call<R: Decodable>(
        _ method: String,
        params: [String: AnyJSON] = [:],
        timeout: Duration = .seconds(15),
        as: R.Type = R.self
    ) async throws -> R {
        try await connect()
        let id = nextId
        nextId += 1
        let req = Request(jsonrpc: "2.0", id: id, method: method, params: params)
        let frame: Data
        do {
            frame = try JSONEncoder().encode(req) + Data([0x0a])
        } catch {
            throw IpcError.encoding(error)
        }

        // Send first, then await the response. We tolerate the response
        // arriving before we park on the continuation: `routeFrame` stashes
        // unmatched frames in `earlyResponses`, and `awaitResponse` drains
        // that buffer when it registers.
        try writeFrame(frame)

        let body = try await withThrowingTaskGroup(of: Data.self) { group in
            group.addTask { [weak self] in
                guard let self else { throw IpcError.connectionClosed }
                return try await self.awaitResponse(id: id)
            }
            group.addTask {
                try await Task.sleep(for: timeout)
                throw IpcError.timeout
            }
            guard let first = try await group.next() else {
                throw IpcError.connectionClosed
            }
            group.cancelAll()
            return first
        }

        do {
            let resp = try JSONDecoder().decode(Response<R>.self, from: body)
            if let err = resp.error {
                throw IpcError.rpcError(code: err.code, message: err.message)
            }
            guard let result = resp.result else {
                throw IpcError.rpcError(code: -32603, message: "missing result")
            }
            return result
        } catch let e as IpcError {
            throw e
        } catch {
            throw IpcError.decoding(error)
        }
    }

    /// Subscribe to broadcast notifications. Calls `subscribe` RPC, then yields
    /// every matching notification until the returned stream is cancelled.
    public func subscribe(events: [String]) async throws -> AsyncStream<DaemonNotification> {
        let _: SubscriptionAck = try await call(
            "subscribe",
            params: ["events": .array(events.map { .string($0) })]
        )
        let id = UUID()
        return AsyncStream { cont in
            Task { [weak self] in
                await self?.registerNotificationStream(id: id, cont: cont)
            }
            cont.onTermination = { [weak self] _ in
                Task { [weak self] in
                    await self?.unregisterNotificationStream(id: id)
                }
            }
        }
    }

    // MARK: - Internals

    /// Suspends until a frame arrives for `id`. If the frame already arrived
    /// before this call (stashed in `earlyResponses`), returns immediately.
    private func awaitResponse(id: UInt64) async throws -> Data {
        if isClosed { throw IpcError.connectionClosed }
        if let early = earlyResponses.removeValue(forKey: id) {
            return early
        }
        return try await withCheckedThrowingContinuation { cont in
            // Register on the actor (we are already on the actor here).
            pending[id] = cont
        }
    }

    private func registerNotificationStream(
        id: UUID,
        cont: AsyncStream<DaemonNotification>.Continuation
    ) {
        if isClosed { cont.finish(); return }
        notificationContinuations[id] = cont
    }

    private func unregisterNotificationStream(id: UUID) {
        notificationContinuations.removeValue(forKey: id)
    }

    /// Synchronous blocking write. Loops on EINTR and short writes.
    private func writeFrame(_ data: Data) throws {
        guard fd >= 0, !isClosed else { throw IpcError.connectionClosed }
        let currentFd = fd
        try data.withUnsafeBytes { (buf: UnsafeRawBufferPointer) -> Void in
            var bytesLeft = data.count
            var ptr = buf.baseAddress!
            while bytesLeft > 0 {
                let written = Darwin.write(currentFd, ptr, bytesLeft)
                if written < 0 {
                    if errno == EINTR { continue }
                    throw IpcError.connectionClosed
                }
                if written == 0 { throw IpcError.connectionClosed }
                bytesLeft -= written
                ptr = ptr.advanced(by: written)
            }
        }
    }

    /// Non-isolated reader. Runs on a detached task so blocking `read(2)` does
    /// not pin the actor's executor; that is what allows `disconnect()` to
    /// close the fd and unblock us via EBADF.
    private static func readLoop(fd: Int32, owner: IpcClient?) async {
        var buffer = Data()
        let chunkSize = 4096
        let chunk = UnsafeMutablePointer<UInt8>.allocate(capacity: chunkSize)
        defer { chunk.deallocate() }
        while !Task.isCancelled {
            let n = Darwin.read(fd, chunk, chunkSize)
            if n <= 0 { break } // EOF or error (e.g. EBADF after disconnect closes the fd)
            buffer.append(chunk, count: n)
            while let nl = buffer.firstIndex(of: 0x0a) {
                let frame = Data(buffer[buffer.startIndex..<nl])
                buffer.removeSubrange(buffer.startIndex...nl)
                await owner?.routeFrame(frame)
            }
        }
        await owner?.failOnEof()
    }

    fileprivate func failOnEof() {
        guard !isClosed else { return }
        isClosed = true
        if fd >= 0 {
            close(fd)
            fd = -1
        }
        for (_, c) in pending { c.resume(throwing: IpcError.connectionClosed) }
        pending.removeAll()
        for (_, c) in notificationContinuations { c.finish() }
        notificationContinuations.removeAll()
    }

    fileprivate func routeFrame(_ data: Data) {
        // Either a response (has "id") or a notification (no id).
        if let id = peekId(data) {
            if let cont = pending.removeValue(forKey: id) {
                cont.resume(returning: data)
            } else {
                // Awaiter hasn't parked yet — stash the response.
                earlyResponses[id] = data
            }
            return
        }
        if let n = try? JSONDecoder().decode(DaemonNotification.self, from: data) {
            for (_, c) in notificationContinuations { c.yield(n) }
        }
    }

    private func peekId(_ data: Data) -> UInt64? {
        struct IdOnly: Decodable { let id: UInt64? }
        return (try? JSONDecoder().decode(IdOnly.self, from: data))?.id
    }
}

// MARK: - Wire types

private struct Request: Encodable {
    let jsonrpc: String
    let id: UInt64
    let method: String
    let params: [String: AnyJSON]
}

private struct Response<R: Decodable>: Decodable {
    let jsonrpc: String?
    let id: UInt64?
    let result: R?
    let error: ErrorBody?
}

private struct ErrorBody: Decodable {
    let code: Int
    let message: String
}

public struct SubscriptionAck: Codable, Sendable {
    public let subscribed: [String]
}

public struct Empty: Codable, Sendable { public init() {} }
