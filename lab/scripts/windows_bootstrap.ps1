$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)
$root = 'C:\kk-l4-native'
if (Test-Path $root) {
    if (-not (Test-Path "$root\qualification-owner.txt")) { throw 'Refusing an unrecognized existing workspace' }
} else {
    New-Item -ItemType Directory -Path $root | Out-Null
    [IO.File]::WriteAllText("$root\qualification-owner.txt", 'K&K win11 native qualification')
}
$adapter = @(Get-NetAdapter | Where-Object MacAddress -eq '52-54-00-73-2D-9A')
if ($adapter.Count -ne 1) { throw 'Declared adapter is not unique' }
$id = $adapter[0].ifIndex
Set-NetIPInterface -InterfaceIndex $id -AddressFamily IPv4 -Dhcp Disabled
$address = @(Get-NetIPAddress -InterfaceIndex $id -AddressFamily IPv4 | Where-Object IPAddress -Like '192.168.123.*')
if ($address.Count -eq 0) {
    # Deliberate temporary guest-only address: no wait for host DHCP or firewall.
    New-NetIPAddress -InterfaceIndex $id -IPAddress '192.168.123.16' -PrefixLength 24 -DefaultGateway '192.168.123.1' | Out-Null
}
if (-not (Get-NetRoute -InterfaceIndex $id -DestinationPrefix '0.0.0.0/0' -ErrorAction SilentlyContinue)) {
    New-NetRoute -InterfaceIndex $id -DestinationPrefix '0.0.0.0/0' -NextHop '192.168.123.1' | Out-Null
}
# The host's current tunnel passes HTTPS but not ordinary guest DNS. Use the
# guest OS DoH resolver over the already verified numeric HTTPS endpoint.
$doh = Get-DnsClientDohServerAddress -ServerAddress '1.1.1.1' -ErrorAction SilentlyContinue
if ($doh) {
    Set-DnsClientDohServerAddress -ServerAddress '1.1.1.1' -DohTemplate 'https://1.1.1.1/dns-query' -AllowFallbackToUdp $false -AutoUpgrade $true
} else {
    Add-DnsClientDohServerAddress -ServerAddress '1.1.1.1' -DohTemplate 'https://1.1.1.1/dns-query' -AllowFallbackToUdp $false -AutoUpgrade $true
}
Set-DnsClientServerAddress -InterfaceIndex $id -ServerAddresses @('1.1.1.1')
Clear-DnsClientCache
$env:CARGO_HOME = "$root\cargo"
$env:RUSTUP_HOME = "$root\rustup"
$env:CARGO_BUILD_JOBS = '1'
$env:CARGO_PROFILE_DEV_DEBUG = '0'
$env:RUST_BACKTRACE = '0'
$channel = '@KK_RUST_CHANNEL@'
$rustup = "$env:CARGO_HOME\bin\rustup.exe"
if (-not (Test-Path $rustup)) {
    $base = 'https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe'
    Invoke-WebRequest -UseBasicParsing -Uri $base -OutFile "$root\rustup-init.exe" -TimeoutSec 90
    $sumContent = (Invoke-WebRequest -UseBasicParsing -Uri "$base.sha256" -TimeoutSec 30).Content
    $sumText = if ($sumContent -is [byte[]]) { [Text.Encoding]::UTF8.GetString($sumContent) } else { [string]$sumContent }
    $sum = $sumText.Trim().Split(' ')[0]
    if ($sum -cnotmatch '^[0-9a-f]{64}$' -or (Get-FileHash "$root\rustup-init.exe" -Algorithm SHA256).Hash.ToLowerInvariant() -cne $sum) { throw 'Rust installer digest mismatch' }
    & "$root\rustup-init.exe" -y --no-modify-path --profile minimal --default-toolchain $channel
    if ($LASTEXITCODE -ne 0) { throw "Rust bootstrap exit $LASTEXITCODE" }
}
& $rustup toolchain install $channel --profile minimal
if ($LASTEXITCODE -ne 0) { throw "Pinned Rust installation exit $LASTEXITCODE" }
& "$env:CARGO_HOME\bin\rustc.exe" --version --verbose
if ($LASTEXITCODE -ne 0) { throw 'rustc inventory failed' }
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
$vs = if (Test-Path $vswhere) { & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath } else { $null }
if (-not $vs) {
    Invoke-WebRequest -UseBasicParsing -Uri 'https://aka.ms/vs/17/release/vs_BuildTools.exe' -OutFile "$root\vs_BuildTools.exe" -TimeoutSec 90
    $signature = Get-AuthenticodeSignature "$root\vs_BuildTools.exe"
    if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notmatch 'Microsoft Corporation') { throw 'Untrusted Microsoft build-tools installer' }
    $arguments = @('--quiet', '--wait', '--norestart', '--nocache', '--installPath', "$root\BuildTools", '--add', 'Microsoft.VisualStudio.Component.VC.Tools.x86.x64', '--add', 'Microsoft.VisualStudio.Component.Windows11SDK.26100')
    $process = Start-Process -FilePath "$root\vs_BuildTools.exe" -ArgumentList $arguments -PassThru
    $null = $process.Handle
    Write-Output "Owned build-tools installer PID: $($process.Id)"
    if (-not $process.WaitForExit(3600000)) { throw 'Build-tools installer still active after one hour; no duplicate install or forced termination' }
    $process.Refresh()
    $code = $process.ExitCode
    if ($code -notin @(0, 3010)) { throw "MSVC Build Tools exit $code" }
    if ($code -eq 3010) { Write-Output 'BOOTSTRAP REBOOT REQUIRED'; exit 3010 }
}
Write-Output 'BOOTSTRAP COMPLETE: guest-only minimal Rust/MSVC prerequisites'
