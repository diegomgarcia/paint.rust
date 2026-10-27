using Paint.Plugin.Protocol;
using Paint.Plugin.SampleEffects;
using Paint.Plugin.Sdk;
using Xunit;

namespace Paint.PluginHost.Tests;

public sealed class SdkAndProtocolTests
{
    [Fact]
    public void SourceCompiledInvertChangesBgrAndPreservesEveryAlphaValue()
    {
        byte[] source =
        [
            0, 1, 2, 0,
            10, 20, 30, 1,
            100, 150, 200, 127,
            250, 251, 252, 255,
        ];
        var surface = new BgraSurface(2, 2, source);

        new InvertEffect().Apply(surface, CancellationToken.None);

        Assert.Equal(
            new byte[]
            {
                255, 254, 253, 0,
                245, 235, 225, 1,
                155, 105, 55, 127,
                5, 4, 3, 255,
            },
            surface.Pixels);
    }

    [Fact]
    public void BgraSurfaceCopiesInputAndRejectsInvalidLengths()
    {
        byte[] source = [1, 2, 3, 4];
        var surface = new BgraSurface(1, 1, source);
        source[0] = 99;
        Assert.Equal(1, surface.Pixels[0]);
        Assert.Equal(4, surface.Stride);
        Assert.Throws<ArgumentOutOfRangeException>(() => new BgraSurface(0, 1, []));
        Assert.Throws<ArgumentException>(() => new BgraSurface(2, 2, new byte[15]));
    }

    [Fact]
    public async Task ApplyRoundTripUsesExactBgraLayout()
    {
        var message = new Apply(
            41,
            9,
            "Invert",
            new BgraSurface(2, 1, [1, 2, 3, 4, 250, 251, 252, 0]));
        Frame frame = MessageCodec.Encode(message);
        var stream = new MemoryStream();
        await FrameCodec.WriteAsync(stream, frame, CancellationToken.None);
        stream.Position = 0;
        Frame? decodedFrame = await FrameCodec.ReadAsync(stream, CancellationToken.None);
        Apply decoded = Assert.IsType<Apply>(MessageCodec.Decode(decodedFrame!));

        Assert.Equal(message.RequestId, decoded.RequestId);
        Assert.Equal(message.Generation, decoded.Generation);
        Assert.Equal(message.Surface.Pixels, decoded.Surface.Pixels);
        Assert.Equal(26 + "Invert"u8.Length + 8, frame.Payload.Length);
    }

    [Fact]
    public async Task OversizedDeclarationFailsBeforePayloadRead()
    {
        var stream = new MemoryStream(HostProcess.Header(
            ProtocolConstants.Version,
            (byte)MessageKind.Apply,
            checked((uint)ProtocolConstants.MaxPayloadLength + 1)));
        await Assert.ThrowsAsync<ProtocolException>(() =>
            FrameCodec.ReadAsync(stream, CancellationToken.None).AsTask());
    }

    [Fact]
    public async Task WrongVersionAndTruncatedFrameAreRejected()
    {
        var wrongVersion = new MemoryStream(HostProcess.Header(2, (byte)MessageKind.Hello, 0));
        await Assert.ThrowsAsync<ProtocolException>(() =>
            FrameCodec.ReadAsync(wrongVersion, CancellationToken.None).AsTask());

        var truncated = new MemoryStream(HostProcess.Header(1, (byte)MessageKind.Hello, 8)
            .Concat(new byte[3]).ToArray());
        await Assert.ThrowsAsync<ProtocolException>(() =>
            FrameCodec.ReadAsync(truncated, CancellationToken.None).AsTask());
    }
}
