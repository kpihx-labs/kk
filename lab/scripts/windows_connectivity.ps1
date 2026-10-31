$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)
function TcpProbe([string] $Address, [int] $Port) {
    $client = New-Object System.Net.Sockets.TcpClient
    try {
        $operation = $client.BeginConnect($Address, $Port, $null, $null)
        if (-not $operation.AsyncWaitHandle.WaitOne(8000)) { return $false }
        $client.EndConnect($operation)
        return $true
    } catch { return $false } finally { $client.Dispose() }
}
$adapter = @(Get-NetAdapter | Where-Object MacAddress -eq '52-54-00-73-2D-9A')
if ($adapter.Count -ne 1) { throw 'Declared adapter is not unique' }
$id = $adapter[0].ifIndex
$report = [ordered]@{
    addresses = @(Get-NetIPAddress -InterfaceIndex $id -AddressFamily IPv4 | Select-Object IPAddress, PrefixLength, PrefixOrigin)
    routes = @(Get-NetRoute -InterfaceIndex $id -DestinationPrefix '0.0.0.0/0' | Select-Object NextHop, RouteMetric)
    dns = @(Get-DnsClientServerAddress -InterfaceIndex $id -AddressFamily IPv4 | Select-Object ServerAddresses)
    gatewayDnsTcp = (TcpProbe '192.168.123.1' 53)
    publicHttpsTcp = (TcpProbe '1.1.1.1' 443)
}
try {
    $reply = Invoke-WebRequest -UseBasicParsing -Uri 'https://1.1.1.1/dns-query?name=static.rust-lang.org&type=A' -Headers @{accept='application/dns-json'} -TimeoutSec 12
    $report.doh = (($reply.Content | ConvertFrom-Json).Answer | Select-Object name, type, data)
} catch { $report.dohError = $_.Exception.Message }
$report | ConvertTo-Json -Depth 5
