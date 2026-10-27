param(
    [Parameter(Mandatory = $true)][string]$ConfigPath
)

$ErrorActionPreference = 'Stop'
$config = Get-Content -LiteralPath $ConfigPath -Raw | ConvertFrom-Json
$exitPath = [string]$config.exit_path
$tempExitPath = $exitPath + '.tmp'
$exitCode = 125
$wrapperError = $null
$started = [DateTimeOffset]::Now.ToString('o')
try {
    Set-Location -LiteralPath ([string]$config.working_directory)
    foreach ($property in $config.environment.PSObject.Properties) {
        Set-Item -LiteralPath ('Env:' + $property.Name) -Value ([string]$property.Value)
    }
    $arguments = @($config.arguments | ForEach-Object { [string]$_ })
    $utf8 = New-Object System.Text.UTF8Encoding($false)
    [Console]::OutputEncoding = $utf8
    $OutputEncoding = $utf8
    $ErrorActionPreference = 'Continue'
    & ([string]$config.executable) @arguments 1> ([string]$config.stdout_path) 2> ([string]$config.stderr_path)
    $exitCode = $LASTEXITCODE
    if ($null -eq $exitCode) { $exitCode = 125 }
    $ErrorActionPreference = 'Stop'
} catch {
    $wrapperError = $_ | Out-String
    $exitCode = 125
    [System.IO.File]::WriteAllText(([string]$config.wrapper_error_path), $wrapperError, [System.Text.Encoding]::UTF8)
}
$result = [pscustomobject]@{
    schema_version = 1
    exit_code = [int]$exitCode
    started_at = $started
    ended_at = [DateTimeOffset]::Now.ToString('o')
    wrapper_error = $wrapperError
}
[System.IO.File]::WriteAllText($tempExitPath, ($result | ConvertTo-Json -Compress), [System.Text.Encoding]::UTF8)
Move-Item -LiteralPath $tempExitPath -Destination $exitPath -Force
exit $exitCode
