$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)
$root = 'C:\kk-l4-native'
$env:CARGO_HOME = "$root\cargo"
$env:RUSTUP_HOME = "$root\rustup"
$env:CARGO_TARGET_DIR = "$root\target"
$env:CARGO_BUILD_JOBS = '1'
$env:CARGO_PROFILE_DEV_DEBUG = '0'
$env:RUST_TEST_THREADS = '1'
$env:RUST_BACKTRACE = '0'
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
$vs = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (-not $vs) { throw 'Native MSVC toolchain unavailable' }
$developer = Join-Path $vs 'Common7\Tools\VsDevCmd.bat'
$command = 'call "{0}" -no_logo -arch=x64 -host_arch=x64 && "{1}\bin\cargo.exe" test --locked --manifest-path "{2}\repo\Cargo.toml" --test windows_native -- --test-threads=1 --nocapture' -f $developer, $env:CARGO_HOME, $root
& "$env:SystemRoot\System32\cmd.exe" /d /s /c $command
$code = $LASTEXITCODE
Write-Output "TARGETED NATIVE EXIT: $code"
exit $code
