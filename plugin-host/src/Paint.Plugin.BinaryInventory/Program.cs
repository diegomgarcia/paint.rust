using System.Reflection;
using System.Reflection.Metadata;
using System.Reflection.PortableExecutable;
using System.Security.Cryptography;
using System.Text.Json;

if (args.Length != 2 || args[0] != "--assembly")
{
    Console.Error.WriteLine("Usage: Paint.Plugin.BinaryInventory --assembly <named-third-party.dll>");
    Console.Error.WriteLine("Metadata is read as data. The assembly is never loaded or executed.");
    return 64;
}

string path = Path.GetFullPath(args[1]);
string fileName = Path.GetFileName(path);
if (fileName.Contains("PaintDotNet", StringComparison.OrdinalIgnoreCase) ||
    fileName.Contains("Paint.NET", StringComparison.OrdinalIgnoreCase))
{
    Console.Error.WriteLine("Refusing Paint.NET binaries. Name a third-party candidate DLL instead.");
    return 65;
}
if (!string.Equals(Path.GetExtension(path), ".dll", StringComparison.OrdinalIgnoreCase) ||
    !File.Exists(path))
{
    Console.Error.WriteLine("The candidate must be an existing .dll file.");
    return 66;
}

using var stream = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.Read);
string hash = Convert.ToHexString(SHA256.HashData(stream)).ToLowerInvariant();
stream.Position = 0;
using var peReader = new PEReader(stream);
if (!peReader.HasMetadata)
{
    Console.Error.WriteLine("The candidate DLL has no managed metadata.");
    return 67;
}
MetadataReader metadata = peReader.GetMetadataReader();
var types = new List<object>();
foreach (TypeDefinitionHandle handle in metadata.TypeDefinitions)
{
    TypeDefinition type = metadata.GetTypeDefinition(handle);
    TypeAttributes visibility = type.Attributes & TypeAttributes.VisibilityMask;
    if (visibility is not TypeAttributes.Public and not TypeAttributes.NestedPublic)
    {
        continue;
    }
    string namespaceName = metadata.GetString(type.Namespace);
    string name = metadata.GetString(type.Name);
    string fullName = string.IsNullOrEmpty(namespaceName) ? name : $"{namespaceName}.{name}";
    var methods = new List<string>();
    foreach (MethodDefinitionHandle methodHandle in type.GetMethods())
    {
        MethodDefinition method = metadata.GetMethodDefinition(methodHandle);
        if ((method.Attributes & MethodAttributes.MemberAccessMask) == MethodAttributes.Public)
        {
            methods.Add(metadata.GetString(method.Name));
        }
    }
    methods.Sort(StringComparer.Ordinal);
    types.Add(new { fullName, publicMethods = methods });
}

var inventory = new
{
    assembly = fileName,
    sha256 = hash,
    note = "Static metadata inventory only; not a compatibility verdict.",
    publicTypes = types,
};
Console.WriteLine(JsonSerializer.Serialize(inventory, new JsonSerializerOptions { WriteIndented = true }));
return 0;
