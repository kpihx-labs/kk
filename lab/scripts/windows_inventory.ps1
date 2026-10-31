$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)
$tools = @(foreach ($tool in @('cargo.exe', 'rustc.exe', 'rustup.exe', 'git.exe', 'cmake.exe', 'cl.exe', 'pwsh.exe')) {
    $found = Get-Command $tool -ErrorAction SilentlyContinue
    [pscustomobject]@{name=$tool; available=($null -ne $found); path=if ($found) {$found.Source} else {$null}}
})
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
$vs = @()
if (Test-Path -LiteralPath $vswhere) {
    $vs = @(& $vswhere -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -format json | ConvertFrom-Json | Select-Object installationPath, installationVersion, isComplete)
    if ($LASTEXITCODE -ne 0) { throw 'vswhere inventory failed' }
}
$adapter = @(Get-NetAdapter | Where-Object MacAddress -eq '52-54-00-73-2D-9A')
$addresses = @(Get-NetIPAddress -InterfaceIndex $adapter[0].ifIndex -AddressFamily IPv4 | Select-Object IPAddress, PrefixLength)
[ordered]@{tools=$tools; visualStudio=$vs; ipv4=$addresses; powershell=$PSVersionTable.PSVersion.ToString(); workspaceExists=(Test-Path 'C:\kk-l4')} | ConvertTo-Json -Depth 5
