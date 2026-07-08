<#
.SYNOPSIS
  Cuts a new OSTP release and pushes it to the channel that triggers the
  matching GitHub Actions build (see .github/workflows/release.yml).

.DESCRIPTION
  Three release channels, in increasing order of stability:
    alpha      -> pushes the `alpha` branch     -> tag "{version}-alpha"
    pre-release  -> pushes the `pre-release` branch  -> tag "{version}-beta"
    master       -> pushes an actual "v{version}" tag -> real stable release

  Promoting to pre-release/master first fast-forwards that branch to
  `alpha` (--ff-only - this always succeeds cleanly as long as nobody ever
  commits directly to pre-release/master, per CONTRIBUTING.md's branch
  strategy), so a release always ships alpha's latest, not a stale branch.

  Remembers the last {version, branch, prefix} it used in .release-state.json
  at the repo root. Running with no arguments repeats last time's branch and
  prefix, auto-incrementing the patch version. -Switch starts a new version
  line (e.g. 0.3.x -> 0.4.0) without changing branch/prefix. -Branch/-Prefix
  override just that one setting for this run (and become the new default).

.PARAMETER Switch
  Set an exact version (e.g. "0.4.0") instead of auto-incrementing the patch
  of the last released version. Becomes the new baseline for future bare runs.

.PARAMETER Branch
  Which branch to release from: master, pre-release, or alpha.
  Defaults to whatever was used last time (see .release-state.json).

.PARAMETER Prefix
  Tag suffix for non-stable channels: beta or alpha. Ignored (forced empty)
  when -Branch master, since stable releases are bare "vX.Y.Z" tags.
  Defaults to whatever was used last time.

.EXAMPLE
  .\scripts\gha.ps1
  Re-releases the same branch/prefix as last time, with the patch version bumped by 1.

.EXAMPLE
  .\scripts\gha.ps1 -Switch 0.4.0
  Starts releasing the 0.4.x line from now on; this run ships exactly 0.4.0.

.EXAMPLE
  .\scripts\gha.ps1 -Branch pre-release -Prefix beta
  Promotes alpha -> pre-release and ships "{version}-beta".
#>
[CmdletBinding()]
param(
    [string]$Switch,
    [ValidateSet('master', 'beta', 'alpha')]
    [string]$Branch,
    [ValidateSet('beta', 'alpha')]
    [string]$Prefix
)

$ErrorActionPreference = "Stop"

function Write-Step($msg) { Write-Host "==> $msg" -ForegroundColor Cyan }
function Write-Warn2($msg) { Write-Host "!! $msg" -ForegroundColor Yellow }
function Fail($msg) { Write-Host "ERROR: $msg" -ForegroundColor Red; exit 1 }

# -- Locate repo root, regardless of where this script was invoked from ------
$RepoRoot = (git rev-parse --show-toplevel 2>$null)
if (-not $RepoRoot) { Fail "Not inside a git repository." }
Set-Location $RepoRoot

$StateFile = Join-Path $RepoRoot ".release-state.json"

# -- Refuse to run on a dirty tree: this script commits, and an autocommit --
# -- silently sweeping up unrelated WIP changes would be a nasty surprise. --
$dirty = git status --porcelain
if ($dirty) {
    Write-Host $dirty
    Fail "Working tree has uncommitted changes. Commit or stash them first."
}

# -- Load remembered state (branch/prefix/version from the last release) ----
$State = $null
if (Test-Path $StateFile) {
    $State = Get-Content $StateFile -Raw | ConvertFrom-Json
}

$ResolvedBranch = if ($Branch) { $Branch } elseif ($State) { $State.branch } else { "alpha" }
$ResolvedPrefix = if ($Prefix) { $Prefix } elseif ($State) { $State.prefix } else { "alpha" }

# Stable releases are always a bare "vX.Y.Z" tag, never suffixed - master
# never carries a prefix regardless of what was remembered or passed in.
if ($ResolvedBranch -eq "master") {
    if ($Prefix) { Write-Warn2 "-Prefix is ignored for -Branch master (stable releases are bare 'vX.Y.Z' tags)." }
    $ResolvedPrefix = ""
}

# -- Resolve the version: exact via -Switch, else auto-increment the patch --
$CurrentVersion = if ($State) { $State.version } else {
    (Select-String -Path (Join-Path $RepoRoot "Cargo.toml") -Pattern '^version = "([0-9]+\.[0-9]+\.[0-9]+)"').Matches[0].Groups[1].Value
}

if ($Switch) {
    if ($Switch -notmatch '^[0-9]+\.[0-9]+\.[0-9]+$') { Fail "-Switch must be a bare X.Y.Z version, got '$Switch'." }
    $NewVersion = $Switch
} else {
    $parts = $CurrentVersion.Split('.')
    $NewVersion = "{0}.{1}.{2}" -f $parts[0], $parts[1], ([int]$parts[2] + 1)
}

Write-Step "Releasing $NewVersion on '$ResolvedBranch'$(if ($ResolvedPrefix) { " (tag suffix: -$ResolvedPrefix)" } else { " (stable, tag v$NewVersion)" })"

# -- Checkout the target branch, promoting it from alpha first ------------
$CurrentBranch = git rev-parse --abbrev-ref HEAD
if ($CurrentBranch -ne $ResolvedBranch) {
    Write-Step "Checking out $ResolvedBranch"
    git checkout $ResolvedBranch 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) { Fail "Could not check out branch '$ResolvedBranch'." }
}
if ($ResolvedBranch -ne "alpha") {
    Write-Step "Fast-forwarding $ResolvedBranch to alpha (promotion)"
    git merge alpha --ff-only 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) {
        $msg = "'$ResolvedBranch' has diverged from alpha and can't fast-forward. " +
               "Per CONTRIBUTING.md, nothing should ever be committed directly to " +
               "$ResolvedBranch - check what's there before forcing anything."
        Fail $msg
    }
}

# -- Bump the version across every manifest that carries one ----------------
Write-Step "Bumping version $CurrentVersion -> $NewVersion"

function Set-VersionLine($Path, $Pattern, $Replacement) {
    $full = Join-Path $RepoRoot $Path
    $text = Get-Content $full -Raw
    $updated = $text -replace $Pattern, $Replacement
    if ($updated -eq $text) { Fail "Version pattern not found in $Path - refusing to proceed with a stale file." }
    [System.IO.File]::WriteAllText($full, $updated)
}

Set-VersionLine "Cargo.toml" '(?m)^version = "[0-9]+\.[0-9]+\.[0-9]+"' "version = `"$NewVersion`""
Set-VersionLine "ostp-gui/src-tauri/Cargo.toml" '(?m)^version = "[0-9]+\.[0-9]+\.[0-9]+"' "version = `"$NewVersion`""
Set-VersionLine "ostp-gui/src-tauri/tauri.conf.json" '"version": "[0-9]+\.[0-9]+\.[0-9]+"' "`"version`": `"$NewVersion`""
Set-VersionLine "ostp-gui/package.json" '"version": "[0-9]+\.[0-9]+\.[0-9]+"' "`"version`": `"$NewVersion`""

# Flutter build number must increase monotonically (Android versionCode) -
# bump it alongside the version string, don't just rewrite the version part.
$pubspecPath = Join-Path $RepoRoot "ostp-flutter/pubspec.yaml"
$pubspecText = Get-Content $pubspecPath -Raw
if ($pubspecText -match 'version: [0-9]+\.[0-9]+\.[0-9]+\+([0-9]+)') {
    $nextBuild = [int]$Matches[1] + 1
    $pubspecText = $pubspecText -replace 'version: [0-9]+\.[0-9]+\.[0-9]+\+[0-9]+', "version: $NewVersion+$nextBuild"
    [System.IO.File]::WriteAllText($pubspecPath, $pubspecText)
} else {
    Fail "Version pattern not found in ostp-flutter/pubspec.yaml."
}

# -- Refresh Cargo.lock's per-package version entries ------------------------
# ostp-gui/src-tauri is excluded from the main workspace (its own Tauri build
# graph), so it has its own separate Cargo.lock that the main `cargo check`
# below never touches - needs its own pass or it'd drift from Cargo.toml.
Write-Step "Running cargo check to refresh Cargo.lock (main workspace)"
cargo check --workspace --exclude ostp-jni --quiet
if ($LASTEXITCODE -ne 0) { Fail "cargo check failed after the version bump - not committing a broken build." }

Write-Step "Running cargo check to refresh Cargo.lock (ostp-gui/src-tauri)"
Push-Location (Join-Path $RepoRoot "ostp-gui/src-tauri")
cargo check --quiet
$tauriCheckExit = $LASTEXITCODE
Pop-Location
if ($tauriCheckExit -ne 0) { Fail "cargo check failed in ostp-gui/src-tauri after the version bump." }

# -- Persist the new state ---------------------------------------------------
[PSCustomObject]@{
    version = $NewVersion
    branch  = $ResolvedBranch
    prefix  = $ResolvedPrefix
} | ConvertTo-Json | Set-Content $StateFile

# -- Commit -------------------------------------------------------------------
$suffixLabel = if ($ResolvedPrefix) { "-$ResolvedPrefix" } else { "" }
$commitMsg = "chore: release $NewVersion$suffixLabel on $ResolvedBranch"
Write-Step "Committing: $commitMsg"
git add Cargo.toml Cargo.lock ostp-gui/src-tauri/Cargo.toml ostp-gui/src-tauri/Cargo.lock `
    ostp-gui/src-tauri/tauri.conf.json ostp-gui/package.json ostp-flutter/pubspec.yaml `
    .release-state.json
git commit -m $commitMsg | Out-Null

# -- Push: branch push for alpha/pre-release (CI computes the tag itself), -
# -- a real "vX.Y.Z" tag for master (the only path that yields a stable      -
# -- release per release.yml's resolve-channel job).                        -
if ($ResolvedBranch -eq "master") {
    $tag = "v$NewVersion"
    Write-Step "Tagging $tag and pushing master + tag"
    git tag $tag
    git push origin master
    git push origin $tag
} else {
    Write-Step "Pushing $ResolvedBranch"
    git push origin $ResolvedBranch
}

Write-Host ""
Write-Host "Done. Watch the build: https://github.com/ospab/ostp/actions" -ForegroundColor Green
