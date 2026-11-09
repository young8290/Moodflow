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
$reboots = [Collections.Generic.List[string]]::new()
$dllRegistrations = [Collections.Generic.List[object]]::new()
$phase = 'initializing'
$failure = $null
$record = Join-Path $workspace 'Moodflow\dist\install-smoke.json'
New-Item -ItemType Directory -Force -Path (Split-Path -Parent $record) | Out-Null

function Complete-Phase([string]$Name) {
    $completed.Add($Name)
    Write-Host "PASS $Name"
}

function Invoke-Package([string]$File, [string[]]$Arguments) {
    $process = Start-Process -FilePath $File -ArgumentList $Arguments -PassThru -WindowStyle Hidden
    if (-not $process.WaitForExit(240000)) {
        $process.Kill()
        throw 'Isolated installer did not finish within four minutes.'
    }
    if ($process.ExitCode -eq 3010) { $reboots.Add($phase) }
    elseif ($process.ExitCode -ne 0) { throw "Installer operation in $phase returned $($process.ExitCode)." }
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
    if ($dllRegistrations.Count -eq 0) {
        foreach ($architecture in @('x64','x86')) {
            $view = if ($architecture -eq 'x64') { [Microsoft.Win32.RegistryView]::Registry64 } else { [Microsoft.Win32.RegistryView]::Registry32 }
            $hive = [Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::LocalMachine,$view)
            $key = $hive.OpenSubKey('Software\Classes\CLSID\{EF62EE30-5ECF-413A-A476-48D1F29E827C}\InprocServer32')
            try {
                if ($null -eq $key) { throw "Missing installed $architecture COM registration." }
                $dll = [string]$key.GetValue('')
            } finally {
                if ($null -ne $key) { $key.Dispose() }
                $hive.Dispose()
            }
            $source = Join-Path $installDir $(if ($architecture -eq 'x64') { 'moodflow_tsf.dll' } else { 'moodflow_tsf_x86.dll' })
            if ((Get-FileHash -LiteralPath $dll -Algorithm SHA256).Hash -ine (Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash) {
                throw "Installed $architecture COM server differs from the verified payload."
            }
            $rules = (Get-Acl -LiteralPath $dll).GetAccessRules($true,$true,[Security.Principal.SecurityIdentifier])
            $readExecute = [int][Security.AccessControl.FileSystemRights]::ReadAndExecute
            $allowed = @($rules | Where-Object { $_.AccessControlType -eq 'Allow' -and ([int]$_.FileSystemRights -band $readExecute) -eq $readExecute })
            $dllRegistrations.Add([pscustomobject]@{
                architecture = $architecture
                copyMatches = $true
                appPackagesReadExecute = @($allowed | Where-Object { $_.IdentityReference.Value -eq 'S-1-15-2-1' }).Count -gt 0
                restrictedPackagesReadExecute = @($allowed | Where-Object { $_.IdentityReference.Value -eq 'S-1-15-2-2' }).Count -gt 0
            })
            $registration=$dllRegistrations[$dllRegistrations.Count-1]
            if (-not $registration.appPackagesReadExecute -or -not $registration.restrictedPackagesReadExecute) {
                throw "Installed $architecture COM server is not readable and executable by sandbox hosts."
            }
        }
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
        if ((Test-Path -LiteralPath $path) -ne $Present) { throw "Phase $phase expected marker present=$Present : $path" }
    }
    if ([IO.File]::ReadAllText($outside) -cne 'external backup must remain') { throw 'An external backup was modified.' }
}
try {
    $phase = 'fresh-install-unicode-path-all-components'
    Invoke-Package $setupPath @('--silent', '--dir', ('"' + $installDir + '"'), '--datadir', ('"' + $dataDir + '"'))
    Assert-Payload
    Stop-OwnedProcesses
    Complete-Phase $phase
    Seed-Markers
    $phase = 'reinstall-keeps-core-and-hub-tool-data'
    Invoke-Package $setupPath @('--silent', '--dir', ('"' + $installDir + '"'), '--datadir', ('"' + $dataDir + '"'))
    Assert-Payload
    Stop-OwnedProcesses
    Assert-Markers $true
    Complete-Phase $phase
    $phase = 'default-silent-uninstall-keeps-all-user-data'
    Invoke-Package (Join-Path $installDir 'uninstall.exe') @('--silent')
    Assert-Markers $true
    Complete-Phase $phase
    Invoke-Package $setupPath @('--silent', '--dir', ('"' + $installDir + '"'), '--datadir', ('"' + $dataDir + '"'))
    Assert-Payload
    Stop-OwnedProcesses
    $phase = 'uninstall-explicitly-keeps-all-user-data'
    Invoke-Package (Join-Path $installDir 'uninstall.exe') @('--silent', '--keep-user-data')
    Assert-Markers $true
    Complete-Phase $phase
    Invoke-Package $setupPath @('--silent', '--dir', ('"' + $installDir + '"'), '--datadir', ('"' + $dataDir + '"'))
    Assert-Payload
    Stop-OwnedProcesses
    $phase = 'conflicting-data-flags-preserve-user-data'
    Invoke-Package (Join-Path $installDir 'uninstall.exe') @('--silent', '--delete-user-data', '--keep-user-data')
    Assert-Markers $true
    Complete-Phase $phase
    $phase = 'uninstall-delete-choice-clears-owned-tools-keeps-external-backup'
    Invoke-Package $setupPath @('--silent', '--dir', ('"' + $installDir + '"'), '--datadir', ('"' + $dataDir + '"'))
    Assert-Payload
    Stop-OwnedProcesses
    Assert-Markers $true
    Invoke-Package (Join-Path $installDir 'uninstall.exe') @('--silent', '--delete-user-data')
    Assert-Markers $false
    Complete-Phase $phase
} catch {
    $failure = $_.Exception.Message
    throw
} finally {
    Stop-OwnedProcesses
    @{ tests = @($completed); phase = $phase; failure = $failure; rebootRequired = @($reboots);
       dllRegistrations = @($dllRegistrations);
       setupSha256 = (Get-FileHash -LiteralPath $setupPath -Algorithm SHA256).Hash.ToLowerInvariant() } |
        ConvertTo-Json -Depth 3 | Set-Content -LiteralPath $record -Encoding utf8
}
