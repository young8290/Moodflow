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
    # PR checkout builds its synthetic merge. Prove its base adds no file changes
    # beyond the head: ancestor, or an equivalent tree since their common base.
    $pullRequests=@($run.pull_requests)
    if ($pullRequests.Count -ne 1 -or $pullRequests[0].base.sha -notmatch '^[0-9a-f]{40}$') { throw 'Missing source PR base.' }
    & git fetch --no-tags origin $pullRequests[0].base.sha
    if ($LASTEXITCODE -ne 0) { throw 'Cannot inspect source PR base.' }
    & git merge-base --is-ancestor $pullRequests[0].base.sha $run.head_sha
    if ($LASTEXITCODE -ne 0) {
        $common=(& git merge-base $pullRequests[0].base.sha $run.head_sha)
        if ($LASTEXITCODE -ne 0) { throw 'Cannot establish source ancestry.' }
        & git diff --quiet $common $pullRequests[0].base.sha
        if ($LASTEXITCODE -ne 0) { throw 'PR merge differs from its head; rebuild from current sources.' }
    }
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
    function Source-Text([string]$Commit,[string]$Path) {
        $lines=@(& git show ($Commit+':'+$Path))
        if ($LASTEXITCODE -ne 0) { throw 'Cannot inspect changed test source.' }
        return ($lines -join "`n").TrimEnd()
    }
    foreach ($path in $changed) {
        if ($path -ceq '.gitattributes') {
            $before=Source-Text $run.head_sha $path
            $after=Source-Text 'HEAD' $path
            $rule="`n`n# Generated theme contract fixtures use LF on every host.`nmoodflow_input/crates/moodflow-theme/testdata/themes/**/expected.json text eol=lf"
            if ($after -cne ($before+$rule)) { throw 'Checkout rules changed beyond test fixtures; rebuild.' }
            continue
        }
        if ($path -ceq 'moodflow_input/crates/moodflow-ui/src/tooltip.rs') {
            # This file also contains production code. Only the single, known
            # cursor-dependent unit test may change; its entire surroundings match.
            $pattern='(?ms)(?:^    ///[^\n]*\n)*^    #\[test\]\n    fn grace_expiry_on_anchor_keeps_tip\(\) \{.*?^    \}\n'
            $before=Source-Text $run.head_sha $path
            $after=Source-Text 'HEAD' $path
            if ([regex]::Matches($before,$pattern).Count -ne 1 -or [regex]::Matches($after,$pattern).Count -ne 1 -or
                [regex]::Replace($before,$pattern,'') -cne [regex]::Replace($after,$pattern,'')) {
                throw 'Tooltip production code changed; rebuild the package.'
            }
            continue
        }
        if ($path -notmatch '^(docs/(design|test)/|moodflow_tsf/tests/)' -and
            $path -cne '.github/workflows/moodflow-package.yml' -and
            $path -cne 'scripts/moodflow/ci-reuse-payload.ps1' -and
            $path -cne 'scripts/moodflow/ci-tsf-e2e.ps1' -and
            $path -cne 'scripts/moodflow/ci-install-smoke.ps1' -and
            $path -cne 'scripts/moodflow/ci-hub-smoke.ps1' -and
            $path -cne 'scripts/moodflow/ci-hub-stack.cpp' -and
            $path -cne 'scripts/moodflow/ci-hub-ui.mjs' -and
            $path -cne 'moodflow_input/crates/moodflow-engine/tests/engine_manager.rs' -and
            $path -cne 'moodflow_input/crates/moodflow-engine/tests/pinyin_completion.rs' -and
            $path -cne 'moodflow_input/crates/moodflow-engine/tests/pinyin_multipath.rs' -and
            $path -cne 'moodflow_input/crates/moodflow-coordinator/src/buffer_tsf_e2e.rs') {
            throw "Production file changed; rebuild the package: $path"
        }
    }
    New-Item -ItemType Directory -Force -Path dist | Out-Null
    @{sourceRun=$RunId;sourceCommit=$run.head_sha;testCommit=(& git rev-parse HEAD);changedTestFiles=$changed} |
        ConvertTo-Json -Depth 3 | Set-Content -LiteralPath 'dist/reused-payload.json' -Encoding utf8
    Write-Output "Production sources match verified package run $RunId."
} finally { $headers.Clear() }
