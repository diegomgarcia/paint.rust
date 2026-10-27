namespace Paint.Plugin.Sdk;

/// <summary>Clone-owned source API. It is not Paint.NET's binary plugin API.</summary>
public interface ICloneEffect
{
    string Name { get; }

    void Apply(BgraSurface surface, CancellationToken cancellationToken);
}
