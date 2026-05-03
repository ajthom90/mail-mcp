using System.IO.Pipes;
using System.Text;

namespace MailMCP.Tests;

/// <summary>
/// Single-client named-pipe server for IpcClient unit tests. Runs on a
/// background task; canned responses keyed by JSON-RPC method name with
/// <c>$ID</c> placeholder for the request id. Pre-canned notifications can
/// be queued before the client connects and pushed at any time after.
/// </summary>
internal sealed class MockIpcServer : IAsyncDisposable
{
    public string PipeName { get; }
    public List<string> ReceivedLines { get; } = new();
    public Dictionary<string, string> Responses { get; } = new();

    private readonly NamedPipeServerStream _pipe;
    private readonly CancellationTokenSource _cts = new();
    // Serializes all writes to _writer. The server's RespondAsync runs on the
    // accept loop; PushNotificationAsync runs on the test thread. Without
    // this both can call StreamWriter.WriteLineAsync concurrently and trip
    // "stream is currently in use by a previous operation."
    private readonly SemaphoreSlim _writeMu = new(1, 1);
    private Task? _loopTask;
    private StreamWriter? _writer;
    private TaskCompletionSource _connected = new(TaskCreationOptions.RunContinuationsAsynchronously);

    public MockIpcServer()
    {
        PipeName = $"mailmcp-test-{Guid.NewGuid():N}";
        _pipe = new NamedPipeServerStream(
            pipeName: PipeName,
            direction: PipeDirection.InOut,
            maxNumberOfServerInstances: 1,
            transmissionMode: PipeTransmissionMode.Byte,
            options: PipeOptions.Asynchronous);
    }

    public string PipeAddress => $@"\\.\pipe\{PipeName}";

    public void Start()
    {
        _loopTask = Task.Run(() => RunAsync(_cts.Token));
    }

    /// <summary>Push a single notification frame to the connected client.</summary>
    public async Task PushNotificationAsync(string frame)
    {
        await _connected.Task.ConfigureAwait(false);
        if (_writer is null) return;
        await WriteLineAsync(frame).ConfigureAwait(false);
    }

    private async Task WriteLineAsync(string frame)
    {
        if (_writer is null) return;
        await _writeMu.WaitAsync().ConfigureAwait(false);
        try
        {
            await _writer.WriteLineAsync(frame).ConfigureAwait(false);
            await _writer.FlushAsync().ConfigureAwait(false);
        }
        finally
        {
            _writeMu.Release();
        }
    }

    private async Task RunAsync(CancellationToken ct)
    {
        await _pipe.WaitForConnectionAsync(ct).ConfigureAwait(false);
        var reader = new StreamReader(_pipe, Encoding.UTF8, leaveOpen: true);
        _writer = new StreamWriter(_pipe, new UTF8Encoding(false), leaveOpen: true)
        {
            NewLine = "\n",
            AutoFlush = false,
        };
        _connected.TrySetResult();
        while (!ct.IsCancellationRequested)
        {
            var line = await reader.ReadLineAsync(ct).ConfigureAwait(false);
            if (line is null) break;
            ReceivedLines.Add(line);
            await RespondAsync(line).ConfigureAwait(false);
        }
    }

    private async Task RespondAsync(string line)
    {
        if (_writer is null) return;
        // Parse the {id, method} fields without pulling in System.Text.Json
        // model classes — keep this mock self-contained.
        var doc = System.Text.Json.JsonDocument.Parse(line);
        var root = doc.RootElement;
        long? id = root.TryGetProperty("id", out var idEl) && idEl.ValueKind == System.Text.Json.JsonValueKind.Number
            ? idEl.GetInt64()
            : null;
        string method = root.TryGetProperty("method", out var mEl)
            ? mEl.GetString() ?? ""
            : "";

        string? template = Responses.TryGetValue(method, out var t) ? t : DefaultFor(method, id);
        if (template is null) return;
        var body = template.Replace("$ID", id?.ToString() ?? "0");
        // Collapse interior newlines so the frame stays one line.
        body = body.Replace("\n", "").Replace("\r", "");
        await WriteLineAsync(body).ConfigureAwait(false);
    }

    private static string? DefaultFor(string method, long? id) => method switch
    {
        "subscribe" => "{\"jsonrpc\":\"2.0\",\"id\":" + (id ?? 0)
                     + ",\"result\":{\"subscribed\":[]}}",
        _ => null,
    };

    public async ValueTask DisposeAsync()
    {
        _cts.Cancel();
        if (_loopTask is not null)
        {
            try { await _loopTask.ConfigureAwait(false); }
            catch (OperationCanceledException) { }
            catch (IOException) { }
        }
        _writer?.Dispose();
        _pipe.Dispose();
        _writeMu.Dispose();
    }
}
