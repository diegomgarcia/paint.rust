if ([string]::IsNullOrEmpty($Root)) { throw 'Root is required' }
if ([string]::IsNullOrEmpty($ExcludeGenerated)) { $ExcludeGenerated = '0' }
$rootItem = Get-Item -LiteralPath $Root
if (-not $rootItem.PSIsContainer) { throw 'Root is not a directory' }
$prefixLength = $rootItem.FullName.TrimEnd('\').Length + 1
$files = @(Get-ChildItem -LiteralPath $rootItem.FullName -Recurse -File | ForEach-Object {
    $relative = $_.FullName.Substring($prefixLength).Replace('\', '/')
    $generated = ($relative -match '(^|/)target(/|$)') -or ($relative -match '^plugin-host/.*/(bin|obj)(/|$)')
    if ($ExcludeGenerated -eq '1' -and $generated) { return }
    [pscustomobject]@{
        path = $relative
        length = [int64]$_.Length
        sha256 = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    }
} | Sort-Object path)
[pscustomobject]@{
    schema_version = 1
    root = $rootItem.FullName
    excluded_generated = ($ExcludeGenerated -eq '1')
    file_count = $files.Count
    files = $files
} | ConvertTo-Json -Depth 5
