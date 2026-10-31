$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)
$root = 'C:\kk-l4-native'
$repo = "$root\repo"
$env:CARGO_HOME = "$root\cargo"
$env:RUSTUP_HOME = "$root\rustup"
$env:CARGO_TARGET_DIR = "$root\target"
$env:CARGO_BUILD_JOBS = '1'
$env:CARGO_PROFILE_DEV_DEBUG = '0'
$env:RUST_TEST_THREADS = '1'
$env:RUST_BACKTRACE = '0'
if (-not (Test-Path "$repo\tests\windows_native.rs")) { throw 'Exact qualification source is missing' }
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
if (-not (Test-Path $vswhere)) { throw 'Native MSVC inventory unavailable' }
$vs = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (-not $vs) { throw 'Native MSVC toolchain unavailable' }
$developer = Join-Path $vs 'Common7\Tools\VsDevCmd.bat'
$cargo = "$env:CARGO_HOME\bin\cargo.exe"
$actor = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
Write-Output "NATIVE ACTOR SID: $actor"
$command = 'call "{0}" -no_logo -arch=x64 -host_arch=x64 && "{1}" test --locked --manifest-path "{2}\Cargo.toml" --test platform_foundation --test windows_native -- --test-threads=1 --nocapture' -f $developer, $cargo, $repo
& "$env:SystemRoot\System32\cmd.exe" /d /s /c $command
if ($LASTEXITCODE -ne 0) { throw "Native Rust gates exit $LASTEXITCODE" }
$previousPreference = $ErrorActionPreference
$ErrorActionPreference = 'Continue'
try {
    $reply = & "$env:SystemRoot\System32\WindowsPowerShell\v1.0\powershell.exe" -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$repo\scripts\install.ps1" -Version 0.0.0 -Commit ('0' * 40) 2>&1
    $installerExit = $LASTEXITCODE
} finally { $ErrorActionPreference = $previousPreference }
if ($installerExit -ne 3 -or ($reply -join "`n") -notmatch 'not shipped') { throw 'Expected noncreating installer refusal was not observed' }
Write-Output 'NATIVE PASS: installer unsupported refusal exit 3 (not installation support)'
$binary = "$env:CARGO_TARGET_DIR\debug\kk.exe"
$hash = (Get-FileHash $binary -Algorithm SHA256).Hash.ToLowerInvariant()
[ordered]@{artifact=$binary; sha256=$hash; actorSid=$actor; windowsRuntime=$true; releaseClaim=$false} | ConvertTo-Json
