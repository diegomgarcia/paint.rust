using System.Collections.Concurrent;
using System.Text;
using Paint.Plugin.Protocol;
using Paint.Plugin.Sdk;

namespace Paint.PluginHost;

public sealed class PluginHostServer
{
    private const int MaxActiveRequests = 64;

    private readonly Stream _input;
    private readonly Stream _output;
    private readonly IReadOnlyDictionary<string, ICloneEffect> _effects;
    private readonly int _diagnosticDelayMilliseconds;
    private readonly SemaphoreSlim _outputLock = new(1, 1);
    private readonly ConcurrentDictionary<ulong, RunningRequest> _active = new();
    private ulong _latestGeneration;

    public PluginHostServer(
        Stream input,
        Stream output,
        IReadOnlyDictionary<string, ICloneEffect> effects,
        int diagnosticDelayMilliseconds = 0)
    {
        ArgumentNullException.ThrowIfNull(input);
        ArgumentNullException.ThrowIfNull(output);
        ArgumentNullException.ThrowIfNull(effects);
        if (diagnosticDelayMilliseconds is < 0 or > 60_000)
        {
            throw new ArgumentOutOfRangeException(
                nameof(diagnosticDelayMilliseconds),
                "The diagnostic delay must be in 0..60000 milliseconds.");
        }
        _input = input;
        _output = output;
        _effects = new Dictionary<string, ICloneEffect>(effects, StringComparer.Ordinal);
        _diagnosticDelayMilliseconds = diagnosticDelayMilliseconds;
    }

    public async Task<int> RunAsync(CancellationToken cancellationToken)
    {
        try
        {
            Frame? firstFrame = await FrameCodec.ReadAsync(_input, cancellationToken)
                .ConfigureAwait(false);
            if (firstFrame is null)
            {
                return 0;
            }
            if (MessageCodec.Decode(firstFrame) is not Hello hello)
            {
                throw new ProtocolException("The first client message must be Hello.");
            }
            string[] effectNames = _effects.Keys.Order(StringComparer.Ordinal).ToArray();
            await WriteMessageAsync(new Ready(hello.RequestId, effectNames)).ConfigureAwait(false);

            while (true)
            {
                Frame? frame = await FrameCodec.ReadAsync(_input, cancellationToken)
                    .ConfigureAwait(false);
                if (frame is null)
                {
                    await CancelAndDrainAsync().ConfigureAwait(false);
                    return 0;
                }
                switch (MessageCodec.Decode(frame))
                {
                    case Apply apply:
                        await DispatchApplyAsync(apply, cancellationToken).ConfigureAwait(false);
                        break;
                    case Cancel cancel:
                        if (_active.TryGetValue(cancel.RequestId, out RunningRequest? running))
                        {
                            running.Cancel();
                        }
                        break;
                    case Shutdown shutdown:
                        await CancelAndDrainAsync().ConfigureAwait(false);
                        await WriteMessageAsync(new ShutdownAck(shutdown.RequestId))
                            .ConfigureAwait(false);
                        return 0;
                    case ProtocolMessage other:
                        throw new ProtocolException(
                            $"Unexpected client message {other.GetType().Name}.");
                }
            }
        }
        finally
        {
            await CancelAndDrainAsync().ConfigureAwait(false);
        }
    }

    private async Task DispatchApplyAsync(Apply apply, CancellationToken cancellationToken)
    {
        if (apply.Generation < _latestGeneration)
        {
            await WriteMessageAsync(new ErrorMessage(
                apply.RequestId,
                apply.Generation,
                RemoteErrorCode.StaleGeneration,
                $"Generation {apply.Generation} is older than {_latestGeneration}."))
                .ConfigureAwait(false);
            return;
        }
        _latestGeneration = apply.Generation;
        if (_active.Count >= MaxActiveRequests)
        {
            await WriteMessageAsync(new ErrorMessage(
                apply.RequestId,
                apply.Generation,
                RemoteErrorCode.InvalidRequest,
                $"No more than {MaxActiveRequests} requests may be active."))
                .ConfigureAwait(false);
            return;
        }

        var running = new RunningRequest(cancellationToken);
        if (!_active.TryAdd(apply.RequestId, running))
        {
            running.Dispose();
            await WriteMessageAsync(new ErrorMessage(
                apply.RequestId,
                apply.Generation,
                RemoteErrorCode.InvalidRequest,
                $"Request ID {apply.RequestId} is already active."))
                .ConfigureAwait(false);
            return;
        }
        running.Completion = ProcessApplyAsync(apply, running);
        await Task.CompletedTask.ConfigureAwait(false);
    }

    private async Task ProcessApplyAsync(Apply apply, RunningRequest running)
    {
        try
        {
            try
            {
                if (!_effects.TryGetValue(apply.Effect, out ICloneEffect? effect))
                {
                    await WriteMessageAsync(new ErrorMessage(
                        apply.RequestId,
                        apply.Generation,
                        RemoteErrorCode.UnsupportedEffect,
                        $"Effect '{apply.Effect}' is not registered."))
                        .ConfigureAwait(false);
                    return;
                }
                if (_diagnosticDelayMilliseconds > 0)
                {
                    await Task.Delay(_diagnosticDelayMilliseconds, running.Cancellation.Token)
                        .ConfigureAwait(false);
                }
                await Task.Run(
                    () => effect.Apply(apply.Surface, running.Cancellation.Token),
                    running.Cancellation.Token).ConfigureAwait(false);
                running.Cancellation.Token.ThrowIfCancellationRequested();
                await WriteMessageAsync(new ApplyResult(
                    apply.RequestId,
                    apply.Generation,
                    apply.Surface)).ConfigureAwait(false);
            }
            catch (OperationCanceledException)
            {
                await WriteMessageAsync(new ErrorMessage(
                    apply.RequestId,
                    apply.Generation,
                    RemoteErrorCode.Cancelled,
                    "The request was cancelled.")).ConfigureAwait(false);
            }
            catch (Exception error)
            {
                await WriteMessageAsync(new ErrorMessage(
                    apply.RequestId,
                    apply.Generation,
                    RemoteErrorCode.Internal,
                    BoundedInternalError(error))).ConfigureAwait(false);
            }
        }
        catch (Exception error) when (
            error is ProtocolException or IOException or ObjectDisposedException)
        {
            // A disconnected broker cannot receive an error frame. The input loop
            // observes EOF and terminates; no background task exception is left unobserved.
        }
        finally
        {
            _active.TryRemove(apply.RequestId, out _);
            running.Dispose();
        }
    }

    private static string BoundedInternalError(Exception error)
    {
        string text = $"{error.GetType().Name}: {error.Message}";
        if (Encoding.UTF8.GetByteCount(text) <= ProtocolConstants.MaxErrorMessageBytes)
        {
            return text;
        }
        var bounded = new StringBuilder();
        int byteCount = 0;
        foreach (Rune rune in text.EnumerateRunes())
        {
            if (byteCount + rune.Utf8SequenceLength > ProtocolConstants.MaxErrorMessageBytes)
            {
                break;
            }
            bounded.Append(rune);
            byteCount += rune.Utf8SequenceLength;
        }
        return bounded.ToString();
    }

    private async Task WriteMessageAsync(ProtocolMessage message)
    {
        await _outputLock.WaitAsync().ConfigureAwait(false);
        try
        {
            await FrameCodec.WriteAsync(
                _output,
                MessageCodec.Encode(message),
                CancellationToken.None).ConfigureAwait(false);
        }
        finally
        {
            _outputLock.Release();
        }
    }

    private async Task CancelAndDrainAsync()
    {
        RunningRequest[] requests = _active.Values.ToArray();
        foreach (RunningRequest request in requests)
        {
            request.Cancel();
        }
        Task[] completions = requests
            .Select(request => request.Completion)
            .OfType<Task>()
            .ToArray();
        if (completions.Length > 0)
        {
            await Task.WhenAll(completions).ConfigureAwait(false);
        }
    }

    private sealed class RunningRequest : IDisposable
    {
        private readonly CancellationTokenSource _cancellation;
        private bool _disposed;

        public RunningRequest(CancellationToken cancellationToken)
        {
            _cancellation = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
        }

        public CancellationTokenSource Cancellation => _cancellation;

        public Task? Completion { get; set; }

        public void Cancel()
        {
            lock (_cancellation)
            {
                if (!_disposed)
                {
                    _cancellation.Cancel();
                }
            }
        }

        public void Dispose()
        {
            lock (_cancellation)
            {
                if (!_disposed)
                {
                    _cancellation.Dispose();
                    _disposed = true;
                }
            }
        }
    }
}
