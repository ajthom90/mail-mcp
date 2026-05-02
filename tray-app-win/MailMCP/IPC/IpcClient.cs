using System.Collections.Concurrent;
using System.IO.Pipes;
using System.Text.Json;

namespace MailMCP.IPC;

/// <summary>
/// JSON-RPC 2.0 client over a Windows named pipe. Newline-delimited frames.
/// Mirrors the v0.1a IpcClient (Rust) and the v0.1b IpcClient (Swift) so all
/// three client implementations agree on the framing and notification
/// semantics.
/// </summary>
public sealed class IpcClient : IAsyncDisposable
{
    public sealed class IpcException : Exception
    {
        public int Code { get; }
        public IpcException(string message, int code = -32000) : base(message) { Code = code; }
    }

    private readonly string _pipePath;
    private NamedPipeClientStream? _pipe;
    private StreamReader? _reader;
    private StreamWriter? _writer;
    private CancellationTokenSource? _readCts;
    private Task? _readTask;
    private long _nextId;

    private readonly ConcurrentDictionary<long, TaskCompletionSource<JsonElement>> _pending = new();
    private readonly List<Channel<DaemonNotification>> _notificationChannels = new();
    private readonly object _channelsLock = new();

    public IpcClient(string pipePath)
    {
        _pipePath = pipePath;
    }

    /// <summary>
    /// Connect to the daemon's pipe. Retries on ERROR_PIPE_BUSY (the daemon's
    /// existing pipe instances are all currently serving other clients) up to
    /// `timeoutMs`.
    /// </summary>
    public async Task ConnectAsync(int timeoutMs = 5000, CancellationToken ct = default)
    {
        if (_pipe?.IsConnected == true) return;

        // The pipe path is `\\.\pipe\<name>`; NamedPipeClientStream wants the
        // bare name plus serverName=".".
        const string Prefix = @"\\.\pipe\";
        var pipeName = _pipePath.StartsWith(Prefix, StringComparison.Ordinal)
            ? _pipePath[Prefix.Length..]
            : _pipePath;

        var pipe = new NamedPipeClientStream(
            serverName: ".",
            pipeName: pipeName,
            direction: PipeDirection.InOut,
            options: PipeOptions.Asynchronous);
        await pipe.ConnectAsync(timeoutMs, ct).ConfigureAwait(false);
        _pipe = pipe;
        _reader = new StreamReader(pipe, leaveOpen: true);
        _writer = new StreamWriter(pipe, leaveOpen: true) { NewLine = "\n", AutoFlush = false };
        _readCts = new CancellationTokenSource();
        _readTask = Task.Run(() => ReadLoopAsync(_readCts.Token));
    }

    /// <summary>
    /// Send a JSON-RPC request and await the matching response. Notifications
    /// (frames without an `id`) are routed to subscribers and don't satisfy
    /// pending requests.
    /// </summary>
    public async Task<JsonElement> CallAsync(
        string method,
        object? @params = null,
        CancellationToken ct = default)
    {
        if (_writer is null) throw new InvalidOperationException("Not connected");
        var id = Interlocked.Increment(ref _nextId);
        var tcs = new TaskCompletionSource<JsonElement>(TaskCreationOptions.RunContinuationsAsynchronously);
        _pending[id] = tcs;

        var req = new
        {
            jsonrpc = "2.0",
            id,
            method,
            @params = @params ?? new { },
        };
        var line = JsonSerializer.Serialize(req);
        await _writer.WriteLineAsync(line.AsMemory(), ct).ConfigureAwait(false);
        await _writer.FlushAsync(ct).ConfigureAwait(false);

        using (ct.Register(() => tcs.TrySetCanceled(ct)))
        {
            return await tcs.Task.ConfigureAwait(false);
        }
    }

    /// <summary>
    /// Strongly-typed convenience overload.
    /// </summary>
    public async Task<T> CallAsync<T>(
        string method,
        object? @params = null,
        CancellationToken ct = default)
    {
        var raw = await CallAsync(method, @params, ct).ConfigureAwait(false);
        return JsonSerializer.Deserialize<T>(raw)
            ?? throw new IpcException($"daemon response for {method} could not be decoded as {typeof(T).Name}");
    }

    /// <summary>
    /// Subscribe to daemon notifications. Sends the JSON-RPC `subscribe`
    /// request and AWAITS the ack before returning, mirroring the Rust + Swift
    /// fix for issue #6 — notifications arriving on the wire after this
    /// returns are guaranteed to be routed to the returned stream.
    ///
    /// The two-stage signature matters: an `async IAsyncEnumerable` iterator
    /// would defer the subscribe RPC until the caller starts iterating, which
    /// re-opens the same race the issue-#6 fix closes.
    /// </summary>
    public async Task<IAsyncEnumerable<DaemonNotification>> SubscribeAsync(
        string[] events,
        CancellationToken ct = default)
    {
        _ = await CallAsync("subscribe", new { events }, ct).ConfigureAwait(false);

        var channel = Channel<DaemonNotification>.Create();
        lock (_channelsLock) { _notificationChannels.Add(channel); }
        return IterateAsync(channel, ct);
    }

    private async IAsyncEnumerable<DaemonNotification> IterateAsync(
        Channel<DaemonNotification> channel,
        [System.Runtime.CompilerServices.EnumeratorCancellation] CancellationToken ct)
    {
        try
        {
            await foreach (var n in channel.ReadAllAsync(ct).ConfigureAwait(false))
            {
                yield return n;
            }
        }
        finally
        {
            lock (_channelsLock) { _notificationChannels.Remove(channel); }
            channel.Complete();
        }
    }

    public async ValueTask DisposeAsync()
    {
        _readCts?.Cancel();
        try
        {
            if (_readTask is not null) await _readTask.ConfigureAwait(false);
        }
        catch (OperationCanceledException) { }

        // Fail every pending request.
        foreach (var (_, tcs) in _pending)
        {
            tcs.TrySetException(new IpcException("daemon disconnected"));
        }
        _pending.Clear();

        // Close each notification channel.
        lock (_channelsLock)
        {
            foreach (var ch in _notificationChannels) ch.Complete();
            _notificationChannels.Clear();
        }

        _writer?.Dispose();
        _reader?.Dispose();
        if (_pipe is not null) await _pipe.DisposeAsync().ConfigureAwait(false);
    }

    private async Task ReadLoopAsync(CancellationToken ct)
    {
        if (_reader is null) return;
        try
        {
            while (!ct.IsCancellationRequested)
            {
                var line = await _reader.ReadLineAsync(ct).ConfigureAwait(false);
                if (line is null) break;             // EOF
                if (line.Length == 0) continue;
                RouteFrame(line);
            }
        }
        catch (OperationCanceledException) { /* graceful shutdown */ }
        catch (IOException) { /* peer closed */ }
    }

    private void RouteFrame(string line)
    {
        using var doc = JsonDocument.Parse(line);
        var root = doc.RootElement;

        // Response frames have a numeric `id`; notifications do not.
        if (root.TryGetProperty("id", out var idEl)
            && idEl.ValueKind == JsonValueKind.Number
            && idEl.TryGetInt64(out var id)
            && _pending.TryRemove(id, out var tcs))
        {
            if (root.TryGetProperty("error", out var err))
            {
                var code = err.TryGetProperty("code", out var c) ? c.GetInt32() : -32000;
                var msg = err.TryGetProperty("message", out var m)
                    ? (m.GetString() ?? "")
                    : "";
                tcs.TrySetException(new IpcException(msg, code));
            }
            else if (root.TryGetProperty("result", out var result))
            {
                tcs.TrySetResult(result.Clone());
            }
            else
            {
                tcs.TrySetException(new IpcException("response missing result/error"));
            }
            return;
        }

        // Notification.
        try
        {
            var n = DaemonNotification.Decode(root);
            lock (_channelsLock)
            {
                foreach (var ch in _notificationChannels) ch.TryWrite(n);
            }
        }
        catch (JsonException)
        {
            // Drop malformed frames silently; the daemon shouldn't send them.
        }
    }

    /// <summary>
    /// Minimal MPMC channel. The full <c>System.Threading.Channels</c> would do
    /// the same job, but keeping this hand-rolled avoids one more package
    /// reference for what's effectively three methods.
    /// </summary>
    private sealed class Channel<T>
    {
        private readonly object _lock = new();
        private readonly Queue<T> _q = new();
        private TaskCompletionSource<bool>? _waiter;
        private bool _closed;

        public static Channel<T> Create() => new();

        public bool TryWrite(T item)
        {
            lock (_lock)
            {
                if (_closed) return false;
                _q.Enqueue(item);
                _waiter?.TrySetResult(true);
                _waiter = null;
                return true;
            }
        }

        public void Complete()
        {
            lock (_lock)
            {
                _closed = true;
                _waiter?.TrySetResult(false);
                _waiter = null;
            }
        }

        public async IAsyncEnumerable<T> ReadAllAsync(
            [System.Runtime.CompilerServices.EnumeratorCancellation] CancellationToken ct = default)
        {
            while (!ct.IsCancellationRequested)
            {
                T? item;
                Task<bool> wait;
                lock (_lock)
                {
                    if (_q.Count > 0)
                    {
                        item = _q.Dequeue();
                        yield return item!;
                        continue;
                    }
                    if (_closed) yield break;
                    _waiter ??= new TaskCompletionSource<bool>(
                        TaskCreationOptions.RunContinuationsAsynchronously);
                    wait = _waiter.Task;
                }
                using var reg = ct.Register(() =>
                {
                    lock (_lock) _waiter?.TrySetCanceled(ct);
                });
                bool more;
                try { more = await wait.ConfigureAwait(false); }
                catch (OperationCanceledException) { yield break; }
                if (!more) yield break;
            }
        }
    }
}
