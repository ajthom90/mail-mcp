import Foundation
import Network

/// JSON-RPC 2.0 client over a Unix domain socket. Newline-delimited frames.
/// Concurrency: actor — every request and notification is funneled through one task.
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
    private var connection: NWConnection?
    private var nextId: UInt64 = 1
    private var pending: [UInt64: CheckedContinuation<Data, Error>] = [:]
    private var notificationContinuations: [UUID: AsyncStream<DaemonNotification>.Continuation] = [:]
    private var readBuffer = Data()
    private var receiveTask: Task<Void, Never>?

    public init(socketPath: URL) {
        self.socketPath = socketPath
    }

    /// Connect (or no-op if already connected). Throws if the socket is missing.
    public func connect() async throws {
        if connection?.state == .ready { return }
        let endpoint = NWEndpoint.unix(path: socketPath.path)
        let conn = NWConnection(to: endpoint, using: .tcp)   // .tcp is fine for UDS framing
        connection = conn
        try await withCheckedThrowingContinuation { (cont: CheckedContinuation<Void, Error>) in
            conn.stateUpdateHandler = { [weak self] state in
                Task { [weak self] in
                    await self?.handleState(state, connectCont: cont)
                }
            }
            conn.start(queue: .global(qos: .userInitiated))
        }
        startReceiveLoop()
    }

    public func disconnect() {
        connection?.cancel()
        connection = nil
        receiveTask?.cancel()
        receiveTask = nil
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

        let body = try await withThrowingTaskGroup(of: Data.self) { group in
            group.addTask {
                try await withCheckedThrowingContinuation {
                    (cont: CheckedContinuation<Data, Error>) in
                    Task { await self.registerPending(id: id, cont: cont) }
                }
            }
            group.addTask {
                try await Task.sleep(for: timeout)
                throw IpcError.timeout
            }
            try await self.send(frame)
            guard let first = try await group.next() else { throw IpcError.connectionClosed }
            group.cancelAll()
            return first
        }

        // Response is { id, result } or { id, error }
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
        // Send the subscribe RPC and wait for its ack so the daemon's filter is
        // populated before we register our local listener — mirrors the test-side
        // synchronization guarantee from v0.1a IPC server.
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

    private func registerPending(id: UInt64, cont: CheckedContinuation<Data, Error>) {
        pending[id] = cont
    }

    private func registerNotificationStream(
        id: UUID,
        cont: AsyncStream<DaemonNotification>.Continuation
    ) {
        notificationContinuations[id] = cont
    }

    private func unregisterNotificationStream(id: UUID) {
        notificationContinuations.removeValue(forKey: id)
    }

    private func handleState(
        _ state: NWConnection.State,
        connectCont: CheckedContinuation<Void, Error>?
    ) {
        switch state {
        case .ready:
            connectCont?.resume()
        case .failed(let err):
            connectCont?.resume(throwing: IpcError.connectionRefused(err.localizedDescription))
        case .cancelled:
            connectCont?.resume(throwing: IpcError.connectionClosed)
        default:
            break
        }
    }

    private func startReceiveLoop() {
        receiveTask = Task { [weak self] in
            await self?.receiveLoop()
        }
    }

    private func receiveLoop() async {
        guard let conn = connection else { return }
        while !Task.isCancelled {
            let chunk: Data? = await withCheckedContinuation { cont in
                conn.receive(minimumIncompleteLength: 1, maximumLength: 8192) {
                    data, _, isComplete, error in
                    if let error {
                        // Treat any error as EOF for our purposes.
                        _ = error
                        cont.resume(returning: nil)
                    } else if isComplete {
                        cont.resume(returning: data)
                    } else {
                        cont.resume(returning: data)
                    }
                }
            }
            guard let chunk, !chunk.isEmpty else {
                // EOF or error — fail every pending request.
                for (_, c) in pending { c.resume(throwing: IpcError.connectionClosed) }
                pending.removeAll()
                for (_, c) in notificationContinuations { c.finish() }
                notificationContinuations.removeAll()
                return
            }
            readBuffer.append(chunk)
            while let nlIdx = readBuffer.firstIndex(of: 0x0a) {
                let frame = readBuffer[readBuffer.startIndex..<nlIdx]
                readBuffer.removeSubrange(readBuffer.startIndex...nlIdx)
                routeFrame(Data(frame))
            }
        }
    }

    private func routeFrame(_ data: Data) {
        // Either a response (has "id") or a notification (has "method" + "jsonrpc" but no "id"
        // semantically — the daemon emits {jsonrpc, method, params} with no id).
        if let id = peekId(data) {
            if let cont = pending.removeValue(forKey: id) {
                cont.resume(returning: data)
            }
            return
        }
        // Notification.
        if let n = try? JSONDecoder().decode(DaemonNotification.self, from: data) {
            for (_, c) in notificationContinuations { c.yield(n) }
        }
    }

    private func peekId(_ data: Data) -> UInt64? {
        struct IdOnly: Decodable { let id: UInt64? }
        return (try? JSONDecoder().decode(IdOnly.self, from: data))?.id
    }

    private func send(_ frame: Data) async throws {
        guard let conn = connection else { throw IpcError.connectionClosed }
        try await withCheckedThrowingContinuation {
            (cont: CheckedContinuation<Void, Error>) in
            conn.send(content: frame, completion: .contentProcessed { error in
                if let error { cont.resume(throwing: IpcError.connectionRefused(error.localizedDescription)) }
                else { cont.resume() }
            })
        }
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

// Conform empty-response helper.
public struct Empty: Codable, Sendable { public init() {} }
