using System.Buffers.Binary;
using System.Diagnostics;
using Paint.Plugin.Protocol;
using Xunit;

namespace Paint.PluginHost.Tests;

internal sealed class HostProcess : IAsyncDisposable
{
    private readonly Process _process;

    private HostProcess(Process process)
    {
        _process = process;
    }

    public static async Task<HostProcess> StartAsync(
        bool handshake = true,
        params string[] arguments)
    {
        string baseDirectory = AppContext.BaseDirectory;
        string configuration = new DirectoryInfo(
            baseDirectory.TrimEnd(Path.DirectorySeparatorChar)).Parent!.Name;
        string testProjectDirectory = Path.GetFullPath(
            Path.Combine(baseDirectory, "..", "..", ".."));
        string pluginHostRoot = Path.GetFullPath(
            Path.Combine(testProjectDirectory, "..", ".."));
        string hostPath = Path.Combine(
            pluginHostRoot,
            "src",
            "Paint.PluginHost",
            "bin",
            configuration,
            "net10.0",
            "Paint.PluginHost.dll");
        Assert.True(
            File.Exists(hostPath),
            $"The real console host is missing at {hostPath}. Build the solution before running tests.");

        var startInfo = new ProcessStartInfo
        {
            FileName = "dotnet",
            UseShellExecute = false,
            RedirectStandardInput = true,
            RedirectStandardOutput = true,
            RedirectStandardError = false,
        };
        startInfo.ArgumentList.Add(hostPath);
        foreach (string argument in arguments)
        {
            startInfo.ArgumentList.Add(argument);
        }

        var process = Process.Start(startInfo);
        Assert.NotNull(process);
        var host = new HostProcess(process);
        try
        {
            if (handshake)
            {
                await host.SendAsync(new Hello(1)).ConfigureAwait(false);
                Ready ready = Assert.IsType<Ready>(await host.ReadAsync().ConfigureAwait(false));
                Assert.Equal(1UL, ready.RequestId);
                Assert.Contains("Invert", ready.Effects);
            }
            return host;
        }
        catch
        {
            await host.DisposeAsync().ConfigureAwait(false);
            throw;
        }
    }

    public async Task SendAsync(ProtocolMessage message)
    {
        using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(10));
        await FrameCodec.WriteAsync(_process.StandardInput.BaseStream, MessageCodec.Encode(message), timeout.Token)
            .ConfigureAwait(false);
    }

    public async Task<ProtocolMessage> ReadAsync()
    {
        using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(10));
        Frame? frame = await FrameCodec.ReadAsync(_process.StandardOutput.BaseStream, timeout.Token)
            .ConfigureAwait(false);
        Assert.True(frame is not null, "The plugin host closed stdout before sending a response.");
        return MessageCodec.Decode(frame!);
    }

    public async Task SendRawAsync(byte[] bytes)
    {
        using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(10));
        await _process.StandardInput.BaseStream.WriteAsync(bytes, timeout.Token).ConfigureAwait(false);
        await _process.StandardInput.BaseStream.FlushAsync(timeout.Token).ConfigureAwait(false);
    }

    public void CloseInput()
    {
        _process.StandardInput.Close();
    }

    public async Task<int> WaitForExitAsync()
    {
        using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(10));
        try
        {
            await _process.WaitForExitAsync(timeout.Token).ConfigureAwait(false);
        }
        catch (OperationCanceledException)
        {
            if (!_process.HasExited)
            {
                _process.Kill(entireProcessTree: true);
                await _process.WaitForExitAsync().ConfigureAwait(false);
            }
            throw new TimeoutException("The plugin host did not exit within 10 seconds.");
        }
        return _process.ExitCode;
    }

    public static byte[] Header(ushort version, byte kind, uint payloadLength)
    {
        var header = new byte[ProtocolConstants.HeaderLength];
        "PRBR"u8.CopyTo(header);
        BinaryPrimitives.WriteUInt16LittleEndian(header.AsSpan(4, 2), version);
        header[6] = kind;
        BinaryPrimitives.WriteUInt32LittleEndian(header.AsSpan(8, 4), payloadLength);
        return header;
    }

    public async ValueTask DisposeAsync()
    {
        try
        {
            if (!_process.HasExited)
            {
                _process.Kill(entireProcessTree: true);
                await _process.WaitForExitAsync().ConfigureAwait(false);
            }
        }
        finally
        {
            _process.Dispose();
        }
    }
}
