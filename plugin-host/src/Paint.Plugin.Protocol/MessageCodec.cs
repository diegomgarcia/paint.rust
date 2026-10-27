using System.Buffers;
using System.Buffers.Binary;
using System.Text;
using Paint.Plugin.Sdk;

namespace Paint.Plugin.Protocol;

public static class MessageCodec
{
    private static readonly UTF8Encoding StrictUtf8 = new(false, true);

    public static Frame Encode(ProtocolMessage message)
    {
        ArgumentNullException.ThrowIfNull(message);
        ValidateRequestId(message.RequestId);
        var buffer = new ArrayBufferWriter<byte>();
        MessageKind kind;
        switch (message)
        {
            case Apply apply:
                kind = MessageKind.Apply;
                byte[] effect = EncodeString(
                    apply.Effect,
                    ProtocolConstants.MaxEffectNameBytes,
                    "effect name");
                WriteUInt64(buffer, apply.RequestId);
                WriteUInt64(buffer, apply.Generation);
                WriteUInt16(buffer, checked((ushort)effect.Length));
                WriteUInt32(buffer, checked((uint)apply.Surface.Width));
                WriteUInt32(buffer, checked((uint)apply.Surface.Height));
                WriteBytes(buffer, effect);
                WriteBytes(buffer, apply.Surface.Pixels);
                break;
            case Cancel:
                kind = MessageKind.Cancel;
                WriteUInt64(buffer, message.RequestId);
                break;
            case ApplyResult result:
                kind = MessageKind.ApplyResult;
                WriteUInt64(buffer, result.RequestId);
                WriteUInt64(buffer, result.Generation);
                WriteUInt32(buffer, checked((uint)result.Surface.Width));
                WriteUInt32(buffer, checked((uint)result.Surface.Height));
                WriteBytes(buffer, result.Surface.Pixels);
                break;
            case ErrorMessage error:
                kind = MessageKind.Error;
                byte[] errorText = EncodeString(
                    error.Message,
                    ProtocolConstants.MaxErrorMessageBytes,
                    "error message");
                if (!Enum.IsDefined(error.Code))
                {
                    throw new ProtocolException($"Unknown remote error code {(ushort)error.Code}.");
                }
                WriteUInt64(buffer, error.RequestId);
                WriteUInt64(buffer, error.Generation);
                WriteUInt16(buffer, (ushort)error.Code);
                WriteUInt16(buffer, 0);
                WriteUInt32(buffer, checked((uint)errorText.Length));
                WriteBytes(buffer, errorText);
                break;
            case Hello:
                kind = MessageKind.Hello;
                WriteUInt64(buffer, message.RequestId);
                break;
            case Ready ready:
                kind = MessageKind.Ready;
                ArgumentNullException.ThrowIfNull(ready.Effects);
                if (ready.Effects.Count is 0 or > ProtocolConstants.MaxEffects)
                {
                    throw new ProtocolException(
                        $"Ready effect count must be in 1..={ProtocolConstants.MaxEffects}.");
                }
                WriteUInt64(buffer, ready.RequestId);
                WriteUInt16(buffer, checked((ushort)ready.Effects.Count));
                WriteUInt16(buffer, 0);
                foreach (string name in ready.Effects)
                {
                    byte[] effectName = EncodeString(
                        name,
                        ProtocolConstants.MaxEffectNameBytes,
                        "effect name");
                    WriteUInt16(buffer, checked((ushort)effectName.Length));
                    WriteBytes(buffer, effectName);
                }
                break;
            case Shutdown:
                kind = MessageKind.Shutdown;
                WriteUInt64(buffer, message.RequestId);
                break;
            case ShutdownAck:
                kind = MessageKind.ShutdownAck;
                WriteUInt64(buffer, message.RequestId);
                break;
            default:
                throw new ProtocolException($"Unsupported message type {message.GetType().FullName}.");
        }
        return new Frame(kind, buffer.WrittenSpan.ToArray());
    }

    public static ProtocolMessage Decode(Frame frame)
    {
        ArgumentNullException.ThrowIfNull(frame);
        ArgumentNullException.ThrowIfNull(frame.Payload);
        var reader = new PayloadReader(frame.Payload);
        ProtocolMessage message;
        switch (frame.Kind)
        {
            case MessageKind.Apply:
            {
                ulong requestId = reader.ReadUInt64();
                ulong generation = reader.ReadUInt64();
                int nameLength = reader.ReadUInt16();
                int width = ReadDimension(reader.ReadUInt32(), "width");
                int height = ReadDimension(reader.ReadUInt32(), "height");
                string effect = ReadString(
                    ref reader,
                    nameLength,
                    ProtocolConstants.MaxEffectNameBytes,
                    "effect name");
                int pixelLength = PixelLength(width, height);
                EnsureRemaining(ref reader, pixelLength, "apply pixel buffer");
                var surface = new BgraSurface(width, height, reader.ReadBytes(pixelLength).ToArray());
                message = new Apply(requestId, generation, effect, surface);
                break;
            }
            case MessageKind.Cancel:
                message = new Cancel(reader.ReadUInt64());
                break;
            case MessageKind.ApplyResult:
            {
                ulong requestId = reader.ReadUInt64();
                ulong generation = reader.ReadUInt64();
                int width = ReadDimension(reader.ReadUInt32(), "width");
                int height = ReadDimension(reader.ReadUInt32(), "height");
                int pixelLength = PixelLength(width, height);
                EnsureRemaining(ref reader, pixelLength, "result pixel buffer");
                var surface = new BgraSurface(width, height, reader.ReadBytes(pixelLength).ToArray());
                message = new ApplyResult(requestId, generation, surface);
                break;
            }
            case MessageKind.Error:
            {
                ulong requestId = reader.ReadUInt64();
                ulong generation = reader.ReadUInt64();
                ushort numericCode = reader.ReadUInt16();
                if (!Enum.IsDefined((RemoteErrorCode)numericCode))
                {
                    throw new ProtocolException($"Unknown remote error code {numericCode}.");
                }
                if (reader.ReadUInt16() != 0)
                {
                    throw new ProtocolException("Error reserved field must be zero.");
                }
                uint messageLength = reader.ReadUInt32();
                if (messageLength > int.MaxValue)
                {
                    throw new ProtocolException("Error message length exceeds the integer bound.");
                }
                string text = ReadString(
                    ref reader,
                    (int)messageLength,
                    ProtocolConstants.MaxErrorMessageBytes,
                    "error message");
                message = new ErrorMessage(
                    requestId,
                    generation,
                    (RemoteErrorCode)numericCode,
                    text);
                break;
            }
            case MessageKind.Hello:
                message = new Hello(reader.ReadUInt64());
                break;
            case MessageKind.Ready:
            {
                ulong requestId = reader.ReadUInt64();
                int count = reader.ReadUInt16();
                if (reader.ReadUInt16() != 0)
                {
                    throw new ProtocolException("Ready reserved field must be zero.");
                }
                if (count is 0 or > ProtocolConstants.MaxEffects)
                {
                    throw new ProtocolException(
                        $"Ready effect count must be in 1..={ProtocolConstants.MaxEffects}.");
                }
                var effects = new string[count];
                for (int index = 0; index < effects.Length; index++)
                {
                    effects[index] = ReadString(
                        ref reader,
                        reader.ReadUInt16(),
                        ProtocolConstants.MaxEffectNameBytes,
                        "effect name");
                }
                message = new Ready(requestId, effects);
                break;
            }
            case MessageKind.Shutdown:
                message = new Shutdown(reader.ReadUInt64());
                break;
            case MessageKind.ShutdownAck:
                message = new ShutdownAck(reader.ReadUInt64());
                break;
            default:
                throw new ProtocolException($"Unknown plugin message kind {(byte)frame.Kind}.");
        }
        reader.Finish();
        ValidateRequestId(message.RequestId);
        return message;
    }

    private static int PixelLength(int width, int height)
    {
        long length = checked((long)width * height * 4);
        if (length > BgraSurface.MaxPixelBytes)
        {
            throw new ProtocolException(
                $"Pixel buffer length {length} exceeds the {BgraSurface.MaxPixelBytes}-byte bound.");
        }
        return checked((int)length);
    }

    private static int ReadDimension(uint value, string label)
    {
        if (value == 0 || value > int.MaxValue)
        {
            throw new ProtocolException($"Invalid {label} {value}.");
        }
        return (int)value;
    }

    private static void EnsureRemaining(ref PayloadReader reader, int expected, string label)
    {
        if (reader.Remaining != expected)
        {
            throw new ProtocolException(
                $"The {label} has {reader.Remaining} bytes; expected exactly {expected}.");
        }
    }

    private static string ReadString(
        ref PayloadReader reader,
        int length,
        int maximum,
        string label)
    {
        if (length is 0 || length > maximum)
        {
            throw new ProtocolException(
                $"The {label} byte length must be in 1..={maximum}, got {length}.");
        }
        try
        {
            return StrictUtf8.GetString(reader.ReadBytes(length));
        }
        catch (DecoderFallbackException error)
        {
            throw new ProtocolException($"The {label} is not valid UTF-8.", error);
        }
    }

    private static byte[] EncodeString(string value, int maximum, string label)
    {
        if (string.IsNullOrEmpty(value))
        {
            throw new ProtocolException($"The {label} must not be empty.");
        }
        byte[] bytes = StrictUtf8.GetBytes(value);
        if (bytes.Length > maximum)
        {
            throw new ProtocolException(
                $"The {label} byte length {bytes.Length} exceeds {maximum}.");
        }
        return bytes;
    }

    private static void ValidateRequestId(ulong requestId)
    {
        if (requestId == 0)
        {
            throw new ProtocolException("Request IDs must be nonzero.");
        }
    }

    private static void WriteUInt16(ArrayBufferWriter<byte> buffer, ushort value)
    {
        BinaryPrimitives.WriteUInt16LittleEndian(buffer.GetSpan(2), value);
        buffer.Advance(2);
    }

    private static void WriteUInt32(ArrayBufferWriter<byte> buffer, uint value)
    {
        BinaryPrimitives.WriteUInt32LittleEndian(buffer.GetSpan(4), value);
        buffer.Advance(4);
    }

    private static void WriteUInt64(ArrayBufferWriter<byte> buffer, ulong value)
    {
        BinaryPrimitives.WriteUInt64LittleEndian(buffer.GetSpan(8), value);
        buffer.Advance(8);
    }

    private static void WriteBytes(ArrayBufferWriter<byte> buffer, ReadOnlySpan<byte> bytes)
    {
        bytes.CopyTo(buffer.GetSpan(bytes.Length));
        buffer.Advance(bytes.Length);
    }

    private ref struct PayloadReader
    {
        private readonly ReadOnlySpan<byte> _bytes;
        private int _position;

        public PayloadReader(ReadOnlySpan<byte> bytes)
        {
            _bytes = bytes;
            _position = 0;
        }

        public int Remaining => _bytes.Length - _position;

        public ReadOnlySpan<byte> ReadBytes(int count)
        {
            int end = checked(_position + count);
            if (count < 0 || end > _bytes.Length)
            {
                throw new ProtocolException(
                    $"Payload needs {count} bytes but only {Remaining} remain.");
            }
            ReadOnlySpan<byte> result = _bytes[_position..end];
            _position = end;
            return result;
        }

        public ushort ReadUInt16() =>
            BinaryPrimitives.ReadUInt16LittleEndian(ReadBytes(2));

        public uint ReadUInt32() =>
            BinaryPrimitives.ReadUInt32LittleEndian(ReadBytes(4));

        public ulong ReadUInt64() =>
            BinaryPrimitives.ReadUInt64LittleEndian(ReadBytes(8));

        public void Finish()
        {
            if (Remaining != 0)
            {
                throw new ProtocolException($"Payload has {Remaining} trailing bytes.");
            }
        }
    }
}
