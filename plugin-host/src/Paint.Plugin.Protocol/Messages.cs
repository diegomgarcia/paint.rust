using Paint.Plugin.Sdk;

namespace Paint.Plugin.Protocol;

public enum MessageKind : byte
{
    Apply = 1,
    Cancel = 2,
    ApplyResult = 3,
    Error = 4,
    Hello = 5,
    Ready = 6,
    Shutdown = 7,
    ShutdownAck = 8,
}

public enum RemoteErrorCode : ushort
{
    InvalidRequest = 1,
    UnsupportedEffect = 2,
    StaleGeneration = 3,
    Cancelled = 4,
    Internal = 5,
}

public abstract record ProtocolMessage(ulong RequestId);

public sealed record Apply(
    ulong Id,
    ulong Generation,
    string Effect,
    BgraSurface Surface) : ProtocolMessage(Id);

public sealed record Cancel(ulong Id) : ProtocolMessage(Id);

public sealed record ApplyResult(
    ulong Id,
    ulong Generation,
    BgraSurface Surface) : ProtocolMessage(Id);

public sealed record ErrorMessage(
    ulong Id,
    ulong Generation,
    RemoteErrorCode Code,
    string Message) : ProtocolMessage(Id);

public sealed record Hello(ulong Id) : ProtocolMessage(Id);

public sealed record Ready(ulong Id, IReadOnlyList<string> Effects) : ProtocolMessage(Id);

public sealed record Shutdown(ulong Id) : ProtocolMessage(Id);

public sealed record ShutdownAck(ulong Id) : ProtocolMessage(Id);

public sealed record Frame(MessageKind Kind, byte[] Payload);

public sealed class ProtocolException : Exception
{
    public ProtocolException(string message)
        : base(message)
    {
    }

    public ProtocolException(string message, Exception innerException)
        : base(message, innerException)
    {
    }
}

public static class ProtocolConstants
{
    public const ushort Version = 1;
    public const int HeaderLength = 16;
    public const int MaxPayloadLength = BgraSurface.MaxPixelBytes + 64;
    public const int MaxEffectNameBytes = 64;
    public const int MaxErrorMessageBytes = 4 * 1024;
    public const int MaxEffects = 64;

    public static bool IsKnownKind(MessageKind kind) =>
        kind >= MessageKind.Apply && kind <= MessageKind.ShutdownAck;
}
