# Synthetic text only; all registration and token work is restricted to a disposable CI VM.
param([Parameter(Mandatory)][string]$Payload)
$ErrorActionPreference='Stop'
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_OS -ne 'Windows' -or -not $env:RUNNER_TEMP) {
    throw 'Real TSF registration is restricted to an isolated GitHub Windows runner.'
}
$root=(Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '../..')).Path
$payloadDir=(Resolve-Path -LiteralPath $Payload).Path
if ($payloadDir.TrimEnd('\') -ine (Join-Path $root 'build').TrimEnd('\')) { throw 'Use the verified release payload.' }
$registry='HKLM:\Software\Moodflow'
if (Test-Path -LiteralPath $registry) { throw 'Isolated install smoke must finish uninstalling first.' }
cmake -S (Join-Path $root 'moodflow_tsf') -B (Join-Path $root '.cache/tsf-e2e') -G 'Visual Studio 17 2022' -A x64 -DMOODFLOW_TSF_E2E=ON
if ($LASTEXITCODE -ne 0) { throw 'Cannot configure the real TSF acceptance host.' }
cmake --build (Join-Path $root '.cache/tsf-e2e') --target tsf_e2e_host --config Release --parallel 2
if ($LASTEXITCODE -ne 0) { throw 'Cannot compile the real TSF acceptance host.' }
$fixture=Join-Path $env:RUNNER_TEMP ('Moodflow TSF E2E '+[guid]::NewGuid().ToString('N'))
$bin=Join-Path $fixture 'bin'
$reports=Join-Path $fixture 'reports'
New-Item -ItemType Directory -Path $bin,$reports | Out-Null
Copy-Item -LiteralPath (Join-Path $root '.cache/tsf-e2e/e2e/Release/tsf_e2e_host.exe') -Destination $bin
Copy-Item -LiteralPath (Join-Path $payloadDir 'moodflow_tsf.dll') -Destination $bin
foreach ($name in @('ordinary','appcontainer','lpac')) { New-Item -ItemType Directory -Path (Join-Path $reports $name) | Out-Null }
# Grant only the public test binaries and synthetic report directory, never the repository,
# build cache, Cargo credentials or any user data. LPAC does not receive the AC alias grant.
& icacls $bin /grant '*S-1-15-2-1:(OI)(CI)RX' '*S-1-15-2-2:(OI)(CI)RX' /T /C | Out-Host
if ($LASTEXITCODE -ne 0) { throw 'Cannot grant public fixture access.' }
& icacls $reports /grant '*S-1-15-2-1:(OI)(CI)M' '*S-1-15-2-2:(OI)(CI)M' /T /C | Out-Host
if ($LASTEXITCODE -ne 0) { throw 'Cannot grant synthetic report access.' }
& icacls $reports /setintegritylevel '(OI)(CI)L' /T /C | Out-Host
if ($LASTEXITCODE -ne 0) { throw 'Cannot label synthetic reports for low integrity.' }
$dll=Join-Path $bin 'moodflow_tsf.dll'
$registered=$false
try {
    New-Item -Path $registry | Out-Null
    Set-ItemProperty -LiteralPath $registry -Name InstallDir -Value $bin
    $process=Start-Process -FilePath "$env:WINDIR\System32\regsvr32.exe" -ArgumentList @('/s',('"'+$dll+'"')) -PassThru -WindowStyle Hidden -Wait
    if ($process.ExitCode -ne 0) { throw 'Isolated TSF registration failed.' }
    $registered=$true
    $env:MOODFLOW_TSF_E2E_HOST=Join-Path $bin 'tsf_e2e_host.exe'
    $env:MOODFLOW_TSF_E2E_DATA=Join-Path $payloadDir 'data'
    $env:MOODFLOW_TSF_E2E_REPORTS=$reports
    $env:MOODFLOW_TSF_E2E_USER=Join-Path $fixture 'private-user'
    $env:CARGO_BUILD_JOBS='2'
    Push-Location (Join-Path $root 'moodflow_input')
    try {
        cargo test --locked -p moodflow-coordinator --lib actual_tsf_buffer_engine_desktop_appcontainer_and_lpac -- --ignored --test-threads=1 --nocapture 2>&1 |
            Tee-Object -FilePath (Join-Path $root 'dist/tsf-e2e.log') | Out-Host
        if ($LASTEXITCODE -ne 0) { throw 'Real TSF end-to-end acceptance failed.' }
    } finally { Pop-Location }
} finally {
    if ($registered) {
        Start-Process -FilePath "$env:WINDIR\System32\regsvr32.exe" -ArgumentList @('/u','/s',('"'+$dll+'"')) -WindowStyle Hidden -Wait
    }
    if (Test-Path -LiteralPath $registry) { Remove-Item -LiteralPath $registry -Force }
    foreach ($name in @('ordinary','appcontainer','lpac')) {
        $source=Join-Path $reports $name
        Copy-Item -LiteralPath $source -Destination (Join-Path $root "dist/tsf-e2e-$name") -Recurse -Force
    }
}
