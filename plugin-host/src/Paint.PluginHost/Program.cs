using Paint.Plugin.Protocol;
using Paint.Plugin.SampleEffects;
using Paint.Plugin.Sdk;
using Paint.PluginHost;

return await MainAsync(args).ConfigureAwait(false);

static async Task<int> MainAsync(string[] args)
{
    int diagnosticDelayMilliseconds = 0;
    for (int index = 0; index < args.Length; index++)
    {
        if (args[index] == "--help")
        {
            Console.Out.WriteLine("Usage: Paint.PluginHost [--diagnostic-delay-ms 0..60000]");
            Console.Out.WriteLine("The diagnostic delay is solely for deterministic cancellation/timeout tests.");
            return 0;
        }
        if (args[index] == "--diagnostic-delay-ms" && index + 1 < args.Length &&
            int.TryParse(args[++index], out diagnosticDelayMilliseconds) &&
            diagnosticDelayMilliseconds is >= 0 and <= 60_000)
        {
            continue;
        }
        Console.Error.WriteLine("Invalid arguments. Use --help for usage.");
        return 64;
    }

    using var shutdown = new CancellationTokenSource();
    ConsoleCancelEventHandler cancelHandler = (_, eventArgs) =>
    {
        eventArgs.Cancel = true;
        shutdown.Cancel();
    };
    Console.CancelKeyPress += cancelHandler;
    try
    {
        var effects = new Dictionary<string, ICloneEffect>(StringComparer.Ordinal)
        {
            [new InvertEffect().Name] = new InvertEffect(),
        };
        var server = new PluginHostServer(
            Console.OpenStandardInput(),
            Console.OpenStandardOutput(),
            effects,
            diagnosticDelayMilliseconds);
        return await server.RunAsync(shutdown.Token).ConfigureAwait(false);
    }
    catch (ProtocolException error)
    {
        Console.Error.WriteLine($"Plugin protocol rejected the connection: {error.Message}");
        return 2;
    }
    catch (OperationCanceledException)
    {
        return 0;
    }
    finally
    {
        Console.CancelKeyPress -= cancelHandler;
    }
}
