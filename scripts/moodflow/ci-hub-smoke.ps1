# Real packaged Hub + core, synthetic data, disposable Windows CI only.
param([Parameter(Mandatory)][string]$Payload)
$ErrorActionPreference='Stop'
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_OS -ne 'Windows' -or
    $env:GITHUB_REPOSITORY -cne 'young8290/Moodflow' -or -not $env:RUNNER_TEMP) {
    throw 'Packaged UI acceptance requires an isolated Windows runner.'
}
$root=(Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '../..')).Path
$payloadDir=(Resolve-Path -LiteralPath $Payload).Path
if ($payloadDir.TrimEnd('\') -ine (Join-Path $root 'build').TrimEnd('\')) { throw 'Use the verified payload.' }
if (Get-Process moodflow_core,moodflow_hub -ErrorAction SilentlyContinue) { throw 'Fresh runner required.' }
$fixture=Join-Path $env:RUNNER_TEMP ('Moodflow Hub UI '+[guid]::NewGuid().ToString('N'))
$bin=Join-Path $fixture 'bin'
New-Item -ItemType Directory -Path $bin | Out-Null
# This copy contains only the verified distribution, never repository credentials.
Get-ChildItem -LiteralPath $payloadDir | Copy-Item -Destination $bin -Recurse
[IO.File]::WriteAllText((Join-Path $bin 'portable_mode'),'')
$user=Join-Path $bin 'localdata'
New-Item -ItemType Directory -Path $user | Out-Null
[IO.File]::WriteAllText((Join-Path $user 'config.toml'),"[moodflow]`nenabled = false`n")
foreach ($name in @('profile','profile/local','profile/roaming','temp','webview')) {
    New-Item -ItemType Directory -Path (Join-Path $fixture $name) | Out-Null
}
$listener=[Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback,0)
$listener.Start()
$port=$listener.LocalEndpoint.Port
$listener.Stop()
function Start-Owned([string]$Name,[string[]]$Arguments) {
    $info=[Diagnostics.ProcessStartInfo]::new((Join-Path $bin $Name))
    $info.UseShellExecute=$false
    $info.CreateNoWindow=$true
    $info.WindowStyle=[Diagnostics.ProcessWindowStyle]::Hidden
    $info.WorkingDirectory=$bin
    $info.Environment.Clear()
    foreach ($key in @('SystemRoot','WINDIR','PATH','ProgramFiles','ProgramFiles(x86)','ProgramData','CommonProgramFiles','CommonProgramFiles(x86)')) {
        if ([Environment]::GetEnvironmentVariable($key)) { $info.Environment[$key]=[Environment]::GetEnvironmentVariable($key) }
    }
    $info.Environment['USERPROFILE']=Join-Path $fixture 'profile'
    $info.Environment['LOCALAPPDATA']=Join-Path $fixture 'profile/local'
    $info.Environment['APPDATA']=Join-Path $fixture 'profile/roaming'
    $info.Environment['TEMP']=Join-Path $fixture 'temp'
    $info.Environment['TMP']=Join-Path $fixture 'temp'
    $info.Environment['MF_HUB_DATA_DIR']=Join-Path $user 'hub'
    $info.Environment['WEBVIEW2_USER_DATA_FOLDER']=Join-Path $fixture 'webview'
    $info.Environment['WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS']="--remote-debugging-address=127.0.0.1 --remote-debugging-port=$port"
    foreach ($argument in $Arguments) { $info.ArgumentList.Add($argument) }
    return [Diagnostics.Process]::Start($info)
}
$core=$null
$hub=$null
New-Item -ItemType Directory -Force -Path (Join-Path $root 'dist') | Out-Null
try {
    $core=Start-Owned 'moodflow_core.exe' @()
    $hub=Start-Owned 'moodflow_hub.exe' @('--open','dashboard')
    & node (Join-Path $PSScriptRoot 'ci-hub-ui.mjs') $port (Join-Path $root 'dist/hub-ui-smoke.json')
    if ($LASTEXITCODE -ne 0) { throw 'Packaged Hub UI/native acceptance failed; see bounded synthetic report.' }
} finally {
    [pscustomobject]@{
        coreExited=($null -eq $core -or $core.HasExited)
        hubExited=($null -eq $hub -or $hub.HasExited)
        hubWindowTitle=$(if ($null -ne $hub -and -not $hub.HasExited) { $hub.MainWindowTitle } else { '' })
        hubExitCode=$(if ($null -ne $hub -and $hub.HasExited) { $hub.ExitCode } else { $null })
    } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $root 'dist/hub-ui-processes.json') -Encoding utf8
    foreach ($process in @($hub,$core)) {
        if ($null -ne $process) {
            try {
                if (-not $process.HasExited) { $process.Kill($true); $process.WaitForExit(10000) | Out-Null }
            } finally { $process.Dispose() }
        }
    }
    # Preserve only application diagnostics, never the synthetic vault/profile.
    $logs=Join-Path $user 'logs'
    if (Test-Path -LiteralPath $logs) {
        Copy-Item -LiteralPath $logs -Destination (Join-Path $root 'dist/hub-ui-core-logs') -Recurse -Force
    }
    $logs=Join-Path $user 'hub/logs'
    if (Test-Path -LiteralPath $logs) {
        Copy-Item -LiteralPath $logs -Destination (Join-Path $root 'dist/hub-ui-hub-logs') -Recurse -Force
    }
}
