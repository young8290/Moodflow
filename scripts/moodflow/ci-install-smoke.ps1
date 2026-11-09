# Real install/uninstall acceptance runs only on GitHub's disposable Windows runner.
# Never use this script to mutate the developer's Windows installation.
param([Parameter(Mandatory)][string]$Setup)
$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_OS -ne 'Windows' -or -not $env:RUNNER_TEMP) {
    throw 'This smoke test is restricted to an isolated GitHub Windows runner.'
}
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
if (-not ([Security.Principal.WindowsPrincipal]::new($identity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw 'The isolated runner must have administrator rights.'
}
$workspace = (Resolve-Path -LiteralPath $env:GITHUB_WORKSPACE).Path.TrimEnd('\')
$setupPath = (Resolve-Path -LiteralPath $Setup).Path
if (-not $setupPath.StartsWith($workspace + '\', [StringComparison]::OrdinalIgnoreCase) -or
    (Split-Path -Leaf $setupPath) -notlike 'Moodflow-Setup-*.exe') { throw 'Unexpected setup path.' }
$smokeRoot = Join-Path $env:RUNNER_TEMP ('Moodflow 安装验收 ' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $smokeRoot | Out-Null
$installDir = Join-Path $smokeRoot '应用目录'
$dataDir = Join-Path $smokeRoot '用户资料'
$outside = Join-Path $smokeRoot '外部备份.txt'
[IO.File]::WriteAllText($outside, 'external backup must remain')
$localRoot = Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'Moodflow'
$ownedTools = @('hub', 'capsule', 'vault', 'capture', 'workspace', 'sync')
$completed = [Collections.Generic.List[string]]::new()

function Invoke-Package([string]$File, [string[]]$Arguments) {
    $process = Start-Process -FilePath $File -ArgumentList $Arguments -PassThru -WindowStyle Hidden
    if (-not $process.WaitForExit(240000)) {
        $process.Kill()
        throw 'Isolated installer did not finish within four minutes.'
    }
    if ($process.ExitCode -ne 0) { throw "Installer operation returned $($process.ExitCode)." }
}
function Stop-OwnedProcesses {
    $prefix = $installDir.TrimEnd('\') + '\'
    Get-Process -ErrorAction SilentlyContinue | ForEach-Object {
        try {
            if ($_.Path -and $_.Path.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) {
                Stop-Process -Id $_.Id -Force -ErrorAction Stop
            }
        } catch { }
    }
}
function Assert-Payload {
    foreach ($name in @('moodflow_core.exe', 'moodflow_tsf.dll', 'moodflow_tsf_x86.dll', 'wind_setting.exe',
        'wind_portable.exe', 'moodflow_hub.exe', 'moodflow-capture.exe', 'uninstall.exe', 'data\manifest.json')) {
        if (-not (Test-Path -LiteralPath (Join-Path $installDir $name) -PathType Leaf)) {
            throw "Installed payload is missing $name."
        }
    }
    $registered = (Get-ItemProperty -LiteralPath 'HKLM:\Software\Moodflow' -Name InstallDir).InstallDir
    if ([IO.Path]::GetFullPath($registered).TrimEnd('\') -ine $installDir.TrimEnd('\')) {
        throw 'TSF install-directory registration points elsewhere.'
    }
}
function Seed-Markers {
    New-Item -ItemType Directory -Force -Path $dataDir | Out-Null
    [IO.File]::WriteAllText((Join-Path $dataDir 'config.toml'), "# isolated fixture`n[debug]`nlog_level = 'info'`n")
    [IO.File]::WriteAllText((Join-Path $dataDir 'ci-user-marker.txt'), 'preserve fixture user data')
    foreach ($name in $ownedTools) {
        $dir = Join-Path $localRoot $name
        New-Item -ItemType Directory -Force -Path $dir | Out-Null
        [IO.File]::WriteAllText((Join-Path $dir 'ci-user-marker.txt'), "private fixture $name")
    }
}
function Assert-Markers([bool]$Present) {
    $paths = @((Join-Path $dataDir 'ci-user-marker.txt')) + @($ownedTools | ForEach-Object { Join-Path (Join-Path $localRoot $_) 'ci-user-marker.txt' })
    foreach ($path in $paths) {
        if ((Test-Path -LiteralPath $path) -ne $Present) { throw "User-data retention differs from the selected option: $path" }
    }
    if ([IO.File]::ReadAllText($outside) -cne 'external backup must remain') { throw 'An external backup was modified.' }
}
try {
    Invoke-Package $setupPath @('--silent', '--dir', ('"' + $installDir + '"'), '--datadir', ('"' + $dataDir + '"'))
    Assert-Payload
    Stop-OwnedProcesses
    $completed.Add('fresh-install-unicode-path-all-components')
    Seed-Markers
    Invoke-Package $setupPath @('--silent', '--dir', ('"' + $installDir + '"'), '--datadir', ('"' + $dataDir + '"'))
    Assert-Payload
    Stop-OwnedProcesses
    Assert-Markers $true
    $completed.Add('reinstall-keeps-core-and-hub-tool-data')
    Invoke-Package (Join-Path $installDir 'uninstall.exe') @('--silent', '--keep-user-data')
    Assert-Markers $true
    $completed.Add('uninstall-explicitly-keeps-all-user-data')
    Invoke-Package $setupPath @('--silent', '--dir', ('"' + $installDir + '"'), '--datadir', ('"' + $dataDir + '"'))
    Assert-Payload
    Stop-OwnedProcesses
    Assert-Markers $true
    Invoke-Package (Join-Path $installDir 'uninstall.exe') @('--silent')
    Assert-Markers $false
    $completed.Add('uninstall-delete-choice-clears-owned-tools-keeps-external-backup')
    @{ tests = @($completed); setupSha256 = (Get-FileHash -LiteralPath $setupPath -Algorithm SHA256).Hash.ToLowerInvariant() } |
        ConvertTo-Json -Depth 3 | Set-Content -LiteralPath (Join-Path $workspace 'Moodflow\dist\install-smoke.json') -Encoding utf8
    $completed | ForEach-Object { Write-Host "PASS $_" }
} finally { Stop-OwnedProcesses }
