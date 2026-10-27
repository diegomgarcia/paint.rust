using System.Buffers.Binary;

namespace Paint.Plugin.Protocol;

public static class FrameCodec
{
    public static async ValueTask<Frame?> ReadAsync(
        Stream stream,
        CancellationToken cancellationToken)
    {
        ArgumentNullException.ThrowIfNull(stream);
        var header = new byte[ProtocolConstants.HeaderLength];
        int first;
        try
        {
            first = await stream.ReadAsync(header.AsMemory(0, 1), cancellationToken)
                .ConfigureAwait(false);
        }
        catch (Exception error) when (error is IOException or ObjectDisposedException)
        {
            throw new ProtocolException("Failed to read the plugin frame header.", error);
        }
        if (first == 0)
        {
            return null;
        }
        await ReadExactlyAsync(stream, header.AsMemory(1), cancellationToken).ConfigureAwait(false);

        if (!header.AsSpan(0, 4).SequenceEqual("PRBR"u8))
        {
            throw new ProtocolException("Invalid plugin frame magic.");
        }
        ushort version = BinaryPrimitives.ReadUInt16LittleEndian(header.AsSpan(4, 2));
        if (version != ProtocolConstants.Version)
        {
            throw new ProtocolException(
                $"Unsupported protocol version {version}; expected {ProtocolConstants.Version}.");
        }
        var kind = (MessageKind)header[6];
        if (!ProtocolConstants.IsKnownKind(kind))
        {
            throw new ProtocolException($"Unknown plugin message kind {header[6]}.");
        }
        if (header[7] != 0 || header.AsSpan(12, 4).IndexOfAnyExcept((byte)0) >= 0)
        {
            throw new ProtocolException("Plugin frame flags and reserved bytes must be zero.");
        }
        uint declaredLength = BinaryPrimitives.ReadUInt32LittleEndian(header.AsSpan(8, 4));
        if (declaredLength > ProtocolConstants.MaxPayloadLength)
        {
            throw new ProtocolException(
                $"Payload length {declaredLength} exceeds the {ProtocolConstants.MaxPayloadLength}-byte bound.");
        }

        // Allocation happens only after magic, version, kind, flags, and bounds pass.
        var payload = new byte[declaredLength];
        await ReadExactlyAsync(stream, payload, cancellationToken).ConfigureAwait(false);
        return new Frame(kind, payload);
    }

    public static async ValueTask WriteAsync(
        Stream stream,
        Frame frame,
        CancellationToken cancellationToken)
    {
        ArgumentNullException.ThrowIfNull(stream);
        ArgumentNullException.ThrowIfNull(frame);
        ArgumentNullException.ThrowIfNull(frame.Payload);
        if (!ProtocolConstants.IsKnownKind(frame.Kind))
        {
            throw new ProtocolException($"Unknown plugin message kind {(byte)frame.Kind}.");
        }
        if (frame.Payload.Length > ProtocolConstants.MaxPayloadLength)
        {
            throw new ProtocolException(
                $"Payload length {frame.Payload.Length} exceeds the {ProtocolConstants.MaxPayloadLength}-byte bound.");
        }

        var header = new byte[ProtocolConstants.HeaderLength];
        "PRBR"u8.CopyTo(header);
        BinaryPrimitives.WriteUInt16LittleEndian(
            header.AsSpan(4, 2),
            ProtocolConstants.Version);
        header[6] = (byte)frame.Kind;
        BinaryPrimitives.WriteUInt32LittleEndian(
            header.AsSpan(8, 4),
            checked((uint)frame.Payload.Length));
        try
        {
            await stream.WriteAsync(header, cancellationToken).ConfigureAwait(false);
            await stream.WriteAsync(frame.Payload, cancellationToken).ConfigureAwait(false);
            await stream.FlushAsync(cancellationToken).ConfigureAwait(false);
        }
        catch (Exception error) when (error is IOException or ObjectDisposedException)
        {
            throw new ProtocolException("Failed to write the plugin frame.", error);
        }
    }

    private static async ValueTask ReadExactlyAsync(
        Stream stream,
        Memory<byte> destination,
        CancellationToken cancellationToken)
    {
        while (!destination.IsEmpty)
        {
            int count;
            try
            {
                count = await stream.ReadAsync(destination, cancellationToken).ConfigureAwait(false);
            }
            catch (Exception error) when (error is IOException or ObjectDisposedException)
            {
                throw new ProtocolException("Failed to read the plugin frame.", error);
            }
            if (count == 0)
            {
                throw new ProtocolException("The plugin frame ended before its declared length.");
            }
            destination = destination[count..];
        }
    }
}
