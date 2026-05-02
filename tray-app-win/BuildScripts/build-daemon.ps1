# Builds mail-mcp-daemon.exe and copies it next to the .NET output.
# Invoked from MailMCP.csproj's BuildDaemon target as a pre-build step.

[CmdletBinding()]
param(
    [string]$Configuration = "Debug",
    [string]$Platform = "x64",
    [string]$OutDir = ""
)

$ErrorActionPreference = "Stop"

$ProjectDir = Split-Path -Parent $PSScriptRoot
$WorkspaceRoot = Resolve-Path (Join-Path $ProjectDir "..")
$Daemon = "mail-mcp-daemon"

# Map MSBuild platform → Rust target triple.
switch ($Platform.ToLowerInvariant()) {
    "x64"   { $Target = "x86_64-pc-windows-msvc" }
    "arm64" { $Target = "aarch64-pc-windows-msvc" }
    "anycpu" {
        # AnyCPU isn't meaningful for native binaries — default to host arch.
        if ([System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture -eq [System.Runtime.InteropServices.Architecture]::Arm64) {
            $Target = "aarch64-pc-windows-msvc"
        } else {
            $Target = "x86_64-pc-windows-msvc"
        }
    }
    default { throw "Unsupported MSBuild platform: $Platform" }
}

if (-not $OutDir) {
    throw "OutDir parameter required"
}

# MSBuild's $(OutDir) ends with a trailing backslash. When that's wrapped in
# &quot;...&quot; in the csproj Exec command, PowerShell sees `path\"` and the
# `\` escapes the quote, leaving a literal `"` at the end of the parameter.
# Strip both characters defensively.
$OutDir = $OutDir.TrimEnd('"', '\')

# Resolve cargo. Prefer the rustup-managed toolchain installer's shim
# at $env:USERPROFILE\.cargo\bin\cargo.exe; fall back to PATH.
$CargoExe = Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe"
if (-not (Test-Path $CargoExe)) {
    $resolved = Get-Command cargo -ErrorAction SilentlyContinue
    if ($resolved) { $CargoExe = $resolved.Source }
    else { throw "cargo not found on PATH or under ~\.cargo\bin" }
}

Push-Location $WorkspaceRoot
try {
    & rustup target add $Target | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "rustup target add $Target failed" }

    if ($Configuration -ieq "Release") {
        & $CargoExe build --release --target $Target -p $Daemon
        $BuiltExe = Join-Path $WorkspaceRoot "target\$Target\release\$Daemon.exe"
    } else {
        & $CargoExe build --target $Target -p $Daemon
        $BuiltExe = Join-Path $WorkspaceRoot "target\$Target\debug\$Daemon.exe"
    }
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
} finally {
    Pop-Location
}

if (-not (Test-Path $BuiltExe)) {
    throw "Daemon binary not produced at $BuiltExe"
}

if (-not (Test-Path $OutDir)) {
    New-Item -ItemType Directory -Path $OutDir -Force | Out-Null
}
$Dest = Join-Path $OutDir "$Daemon.exe"
Copy-Item -LiteralPath $BuiltExe -Destination $Dest -Force
Write-Host "Bundled $Daemon -> $Dest"
