# Explicitly trusted native helper adapter; never extracts archives in PowerShell.
[CmdletBinding()]
param(
    [string] $Version,
    [string] $Commit,
    [string] $Prefix,
    [string] $Archive,
    [string] $Bundle,
    [string] $TrustedRoot,
    [string] $Helper,
    [switch] $Rollback,
    [switch] $VerifyOnly,
    [switch] $Help
)
$ErrorActionPreference = 'Stop'
if ($Help) {
    Write-Output @'
Usage: pwsh -File install.ps1 -Version VERSION -Commit COMMIT [-Prefix ABSOLUTE_PATH]
                            [-Archive ABSOLUTE_PATH [-Bundle ABSOLUTE_PATH]]
                            [-TrustedRoot ABSOLUTE_PATH (requires -Bundle)]
                            -Helper ABSOLUTE_ALREADY_TRUSTED_KK [-Rollback]
Native Windows artifacts are not shipped; Windows installation is refused.
Linux PowerShell can invoke an explicitly trusted helper. No hidden dependency install.
Without -Helper, this PowerShell adapter is BLOCKED; Linux users can use install.sh.
'@
    exit 0
}
try {
    if ($Version -cnotmatch '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9A-Za-z.-]+)?$' -or $Version.Length -gt 64) {
        throw 'Version must be three canonical decimal components (no v prefix).'
    }
    if (-not $Rollback -and $Commit -cnotmatch '^[0-9a-f]{40}$') {
        throw 'Commit must be a full lowercase 40-character SHA.'
    }
    if ($Rollback -and ($Commit -or $Archive)) { throw '-Rollback does not accept Commit or Archive.' }
    if ($VerifyOnly -and $Rollback) { throw '-VerifyOnly does not accept -Rollback.' }
    foreach ($Path in @($Prefix, $Archive, $Bundle, $TrustedRoot, $Helper)) {
        if ($Path -and -not [System.IO.Path]::IsPathFullyQualified($Path)) {
            throw 'Paths must be absolute.'
        }
    }
    if ($Bundle -and -not ($Archive -or $Rollback)) { throw '-Bundle requires -Archive or -Rollback.' }
    if ($TrustedRoot -and -not $Bundle) { throw '-TrustedRoot requires -Bundle.' }
    $Platform = [System.Runtime.InteropServices.RuntimeInformation]::OSDescription
    $Architecture = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture
    $Linux = [System.Runtime.InteropServices.RuntimeInformation]::IsOSPlatform([System.Runtime.InteropServices.OSPlatform]::Linux)
    if ($Linux -and $Helper) {
        if ($Architecture -notin @('X64', 'Arm64')) { throw 'No qualified artifact for this CPU.' }
        $Item = Get-Item -LiteralPath $Helper -Force
        if ($Item.PSIsContainer -or ($Item.Attributes -band [System.IO.FileAttributes]::ReparsePoint)) {
            throw 'Helper must be an ordinary already-trusted executable, not a link.'
        }
        if (-not $Prefix) { $Prefix = Join-Path $HOME '.local' }
        $Arguments = @('install', '--version', $Version, '--commit', $Commit, '--prefix', $Prefix)
        if ($Rollback) { $Arguments = @('rollback', '--version', $Version, '--prefix', $Prefix) }
        if ($VerifyOnly) { $Arguments += '--verify-only' }
        if ($Archive) { $Arguments += @('--archive', $Archive) }
        if ($Bundle) { $Arguments += @('--bundle', $Bundle) }
        if ($TrustedRoot) { $Arguments += @('--trusted-root', $TrustedRoot) }
        & $Helper @Arguments
        exit $LASTEXITCODE
    }
    # No Windows target exists in release.py. A ZIP container does not qualify Windows.
    [Console]::Error.WriteLine("kk installer: BLOCKED on $Platform/$Architecture.")
    [Console]::Error.WriteLine('Native Windows releases are not shipped; no qualified PowerShell installation path exists.')
    [Console]::Error.WriteLine('Linux: use install.sh for verified bootstrap, or -Helper ABSOLUTE_ALREADY_TRUSTED_KK. See docs/installers.md.')
    [Console]::Error.WriteLine('Nothing was downloaded, extracted, installed, deleted, or changed.')
    exit 3
} catch {
    [Console]::Error.WriteLine("kk installer: $($_.Exception.Message)")
    exit 1
}
