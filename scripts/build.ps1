<#
.SYNOPSIS
    Checks, tests and builds ReShift.exe.

.DESCRIPTION
    Runs rustfmt (check), clippy (warnings are errors) and the unit tests, then builds the release
    executable into artifacts\ReShift.exe. The C runtime is linked statically, so the exe runs
    without any redistributable. The E2E smoke test is not run here because it takes over the
    keyboard - run it separately (see AGENTS.md).

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File scripts\build.ps1
.EXAMPLE
    powershell -ExecutionPolicy Bypass -File scripts\build.ps1 -SkipChecks
#>
param(
    [switch]$SkipChecks
)

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent

function Invoke-Cargo {
    param([string[]]$Arguments, [string]$What)
    & cargo @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$What failed" }
}

Push-Location $root
try {
    if (-not $SkipChecks) {
        Invoke-Cargo @('fmt', '--all', '--', '--check') 'rustfmt check'
        Invoke-Cargo @('clippy', '--workspace', '--all-targets', '--', '-D', 'warnings') 'clippy'
        Invoke-Cargo @('test', '--workspace') 'Unit tests'
    }

    Invoke-Cargo @('build', '--release', '--package', 'reshift') 'Release build'

    $output = Join-Path $root 'artifacts'
    New-Item -ItemType Directory -Force $output | Out-Null
    $exe = Join-Path $output 'ReShift.exe'
    Copy-Item (Join-Path $root 'target\release\ReShift.exe') $exe -Force
    'Built {0} ({1:N2} MB)' -f $exe, ((Get-Item $exe).Length / 1MB)
}
finally {
    Pop-Location
}
