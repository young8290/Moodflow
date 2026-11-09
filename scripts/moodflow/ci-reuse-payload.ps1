# Reuse a verified package only when every production file is identical.
# This permits isolated TSF fixture fixes without rebuilding unchanged binaries.
param([Parameter(Mandatory)][string]$RunId)
$ErrorActionPreference='Stop'
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_OS -ne 'Windows' -or
    $env:GITHUB_REPOSITORY -cne 'young8290/Moodflow' -or $RunId -notmatch '^[1-9][0-9]{0,18}$') {
    throw 'Payload reuse requires an explicit run in this repository on Windows CI.'
}
if (-not $env:GH_TOKEN) { throw 'Missing read-only Actions token.' }
$headers=@{Authorization="Bearer $env:GH_TOKEN"; Accept='application/vnd.github+json'; 'X-GitHub-Api-Version'='2026-03-10'}
$base="https://api.github.com/repos/young8290/Moodflow/actions/runs/$RunId"
try {
    $run=Invoke-RestMethod -Uri $base -Headers $headers
    if ($run.name -cne 'Moodflow Package' -or $run.event -cne 'pull_request' -or $run.head_sha -notmatch '^[0-9a-f]{40}$' -or
        $run.head_repository.full_name -cne 'young8290/Moodflow') { throw 'Unexpected package source.' }
    $jobs=Invoke-RestMethod -Uri ($base+'/jobs?per_page=100') -Headers $headers
    $built=@($jobs.jobs | Where-Object { $_.name -ceq 'Windows 安装包' -and $_.conclusion -eq 'success' })
    if ($built.Count -ne 1) { throw 'The source package build must have passed all gates.' }
    & git fetch --no-tags origin $run.head_sha
    if ($LASTEXITCODE -ne 0) { throw 'Cannot inspect the source commit.' }
    # The complete production build job must also match, including all external
    # source pins, flags and integrity gates. Manual source builds are excluded
    # above because their installer_ref input is not recorded by this proof.
    function Build-Definition([string]$Commit) {
        $lines=@(& git show ($Commit+':.github/workflows/moodflow-package.yml'))
        if ($LASTEXITCODE -ne 0) { throw 'Missing original build definition.' }
        $active=$false
        $definition=[Collections.Generic.List[string]]::new()
        foreach ($line in $lines) {
            if ($line -ceq '  windows:') { $active=$true }
            elseif ($active -and $line -cmatch '^  [a-zA-Z0-9_-]+:$') { break }
            if ($active -and $line -cne '    if: ${{ !inputs.reuse_run }}') { $definition.Add($line) }
        }
        if (-not $active) { throw 'Missing production build job.' }
        return ($definition -join "`n").TrimEnd()
    }
    if ((Build-Definition $run.head_sha) -cne (Build-Definition 'HEAD')) { throw 'Build inputs changed; rebuild the package.' }
    $changed=@(& git diff --name-only $run.head_sha HEAD)
    if ($LASTEXITCODE -ne 0) { throw 'Cannot compare source files.' }
    foreach ($path in $changed) {
        if ($path -notmatch '^(docs/(design|test)/|moodflow_tsf/tests/)' -and
            $path -cne '.github/workflows/moodflow-package.yml' -and
            $path -notmatch '^scripts/moodflow/ci-[a-z0-9-]+\.ps1$' -and
            $path -cne 'moodflow_input/crates/moodflow-coordinator/src/buffer_tsf_e2e.rs') {
            throw "Production file changed; rebuild the package: $path"
        }
    }
    New-Item -ItemType Directory -Force -Path dist | Out-Null
    @{sourceRun=$RunId;sourceCommit=$run.head_sha;testCommit=(& git rev-parse HEAD);changedTestFiles=$changed} |
        ConvertTo-Json -Depth 3 | Set-Content -LiteralPath 'dist/reused-payload.json' -Encoding utf8
    Write-Output "Production sources match verified package run $RunId."
} finally { $headers.Clear() }
