using Paint.Plugin.Protocol;
using Paint.Plugin.Sdk;
using Xunit;

namespace Paint.PluginHost.Tests;

public sealed class RealHostTests
{
    private static BgraSurface Source() => new(
        2,
        2,
        [
            0, 1, 2, 0,
            10, 20, 30, 1,
            100, 150, 200, 127,
            250, 251, 252, 255,
        ]);

    [Fact]
    public async Task RealConsoleHostReturnsValidatedInvertOutput()
    {
        await using var host = await HostProcess.StartAsync();
        await host.SendAsync(new Apply(2, 7, "Invert", Source()));
        var result = Assert.IsType<ApplyResult>(await host.ReadAsync());

        Assert.Equal(2UL, result.RequestId);
        Assert.Equal(7UL, result.Generation);
        Assert.Equal(
            new byte[]
            {
                255, 254, 253, 0,
                245, 235, 225, 1,
                155, 105, 55, 127,
                5, 4, 3, 255,
            },
            result.Surface.Pixels);
    }

    [Fact]
    public async Task RealConsoleHostRejectsMalformedBody()
    {
        await using var host = await HostProcess.StartAsync();
        await host.SendRawAsync(HostProcess.Header(1, (byte)MessageKind.Apply, 1));
        await host.SendRawAsync(new byte[1]);
        host.CloseInput();
        Assert.NotEqual(0, await host.WaitForExitAsync());
    }

    [Fact]
    public async Task RealConsoleHostRejectsWrongVersion()
    {
        await using var host = await HostProcess.StartAsync(handshake: false);
        await host.SendRawAsync(HostProcess.Header(2, (byte)MessageKind.Hello, 0));
        host.CloseInput();
        Assert.NotEqual(0, await host.WaitForExitAsync());
    }

    [Fact]
    public async Task RealConsoleHostRejectsOversizedFrameWithoutPayload()
    {
        await using var host = await HostProcess.StartAsync(handshake: false);
        await host.SendRawAsync(HostProcess.Header(
            1,
            (byte)MessageKind.Apply,
            checked((uint)ProtocolConstants.MaxPayloadLength + 1)));
        host.CloseInput();
        Assert.NotEqual(0, await host.WaitForExitAsync());
    }

    [Fact]
    public async Task RealConsoleHostRejectsStaleGeneration()
    {
        await using var host = await HostProcess.StartAsync();
        await host.SendAsync(new Apply(2, 8, "Invert", Source()));
        Assert.IsType<ApplyResult>(await host.ReadAsync());

        await host.SendAsync(new Apply(3, 7, "Invert", Source()));
        var error = Assert.IsType<ErrorMessage>(await host.ReadAsync());
        Assert.Equal(3UL, error.RequestId);
        Assert.Equal(RemoteErrorCode.StaleGeneration, error.Code);
    }

    [Fact]
    public async Task RealConsoleHostProcessesCancelWhileApplyIsRunning()
    {
        await using var host = await HostProcess.StartAsync(
            true,
            "--diagnostic-delay-ms",
            "5000");
        await host.SendAsync(new Apply(2, 1, "Invert", Source()));
        await host.SendAsync(new Cancel(2));

        var error = Assert.IsType<ErrorMessage>(await host.ReadAsync());
        Assert.Equal(2UL, error.RequestId);
        Assert.Equal(RemoteErrorCode.Cancelled, error.Code);
    }

    [Fact]
    public async Task RealConsoleHostReportsUnsupportedEffect()
    {
        await using var host = await HostProcess.StartAsync();
        await host.SendAsync(new Apply(2, 1, "NotRegistered", Source()));
        var error = Assert.IsType<ErrorMessage>(await host.ReadAsync());
        Assert.Equal(RemoteErrorCode.UnsupportedEffect, error.Code);
    }
}
