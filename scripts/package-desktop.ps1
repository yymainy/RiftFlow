[CmdletBinding()]
param([switch]$CheckOnly, [switch]$RefreshIcons)
$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path $PSScriptRoot -Parent
$frontendRoot = Join-Path $repoRoot 'frontend'
$nativeRoot = Join-Path $repoRoot 'riftflow-rs'

function Invoke-Checked([string]$Program, [string[]]$Arguments) {
    & $Program @Arguments
    if ($LASTEXITCODE -ne 0) { throw "Command failed: $Program (exit $LASTEXITCODE)" }
}

Push-Location $repoRoot
try {
    if (-not (Get-Command node -ErrorAction SilentlyContinue)) { throw 'Node.js 24+ is required.' }
    $nodeMajor = [int]((& node -p 'process.versions.node.split(".")[0]').Trim())
    if ($nodeMajor -lt 24) { throw 'Node.js 24+ is required.' }
    if ($CheckOnly) {
        Write-Host '[check] Verifying existing renderer assets.'
        Invoke-Checked 'node' @('frontend/scripts/verify-desktop-assets.mjs')
        return
    }
    $cargoCommand = Get-Command cargo -ErrorAction SilentlyContinue
    $cargo = if ($cargoCommand) { $cargoCommand.Source } else { Join-Path $env:USERPROFILE '.cargo/bin/cargo.exe' }
    if (-not (Test-Path -LiteralPath $cargo)) { throw 'Rust stable with the MSVC toolchain is required.' }
    $lockPath = Join-Path $nativeRoot '.desktop-package.lock'
    $lock = [System.IO.File]::Open($lockPath, [System.IO.FileMode]::OpenOrCreate, [System.IO.FileAccess]::ReadWrite, [System.IO.FileShare]::None)
    try {
        Push-Location $frontendRoot
        try {
            if (-not (Test-Path -LiteralPath 'node_modules')) {
                Write-Host '[1/7] Installing frontend dependencies from the lockfile.'
                Invoke-Checked 'node' @('.yarn/releases/yarn-4.9.1.cjs', 'install', '--immutable')
            }
            if ($RefreshIcons -or -not (Test-Path -LiteralPath 'resources/game-data/manifest.json')) {
                Write-Host '[2/7] Preparing game icons.'
                Invoke-Checked 'python' @('scripts/prepare-game-icons.py')
            }
            Write-Host '[3/7] Checking Vue/TypeScript types.'
            Invoke-Checked 'node' @('--max-old-space-size=4096', 'node_modules/vue-tsc/bin/vue-tsc.js', '--noEmit', '-p', 'tsconfig.web.json', '--composite', 'false')
            Write-Host '[4/7] Building renderer and copying icons.'
            Invoke-Checked 'node' @('--max-old-space-size=6144', 'scripts/build-desktop-renderer.mjs', '../riftflow-rs/renderer-dist')
        } finally { Pop-Location }
        Write-Host '[5/7] Verifying resources before native compilation.'
        $reportText = & node 'frontend/scripts/verify-desktop-assets.mjs'
        if ($LASTEXITCODE -ne 0) { throw 'Renderer validation failed; Cargo was not run.' }
        $report = ($reportText -join "`n") | ConvertFrom-Json
        Push-Location $nativeRoot
        try { Write-Host '[6/7] Building the native executable.'; Invoke-Checked $cargo @('build', '--locked', '--release', '-p', 'riftflow-app', '--bin', 'riftflow') }
        finally { Pop-Location }
        $afterText = & node 'frontend/scripts/verify-desktop-assets.mjs'
        if ($LASTEXITCODE -ne 0) { throw 'Renderer validation failed after compilation.' }
        $after = ($afterText -join "`n") | ConvertFrom-Json
        if ($after.rendererSha256 -ne $report.rendererSha256) { throw 'Renderer changed during native compilation; discard this build and retry.' }
        Write-Host '[7/7] Staging licenses, checksums, and ZIP.'
        $stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
        $output = Join-Path $repoRoot "dist/RiftFlow-$($report.version)-$stamp"
        if (Test-Path -LiteralPath $output) { throw 'Output directory already exists; refusing to overwrite.' }
        New-Item -ItemType Directory -Path $output | Out-Null
        Copy-Item -LiteralPath (Join-Path $nativeRoot 'target/release/riftflow.exe') -Destination (Join-Path $output 'RiftFlow.exe')
        foreach ($item in @('LICENSE', 'THIRD_PARTY_NOTICES.md', 'LICENSES')) {
            Copy-Item -LiteralPath (Join-Path $repoRoot $item) -Destination $output -Recurse
        }
        Copy-Item -LiteralPath (Join-Path $repoRoot 'docs/RELEASE_README.zh-CN.md') -Destination (Join-Path $output 'README.md')
        foreach ($entry in Get-ChildItem -LiteralPath $output -Recurse -File) {
            if ($entry.Name -match '^(ai-config\.json|ai-provider\.json|service\.env|\.env(\..*)?)$|\.(dpapi|db|sqlite|log|pdb|pem|key|p12|pfx)$') { throw "Private/debug file in output: $($entry.Name)" }
        }
        $gitCommit = & git rev-parse HEAD 2>$null
        $report | Add-Member -NotePropertyName sourceCommit -NotePropertyValue ($gitCommit -join '')
        $dirty = & git status --porcelain 2>$null
        $report | Add-Member -NotePropertyName sourceDirty -NotePropertyValue ([bool]$dirty)
        $report | Add-Member -NotePropertyName builtAt -NotePropertyValue ([DateTime]::UtcNow.ToString('o'))
        $report | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $output 'build-report.json') -Encoding UTF8
        Get-ChildItem -LiteralPath $output -Recurse -File | Where-Object { $_.Name -ne 'SHA256SUMS.txt' } | Sort-Object FullName | ForEach-Object {
            $name = $_.FullName.Substring($output.Length + 1).Replace('\', '/')
            '{0}  {1}' -f (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant(), $name
        } | Set-Content -LiteralPath (Join-Path $output 'SHA256SUMS.txt') -Encoding UTF8
        Compress-Archive -LiteralPath $output -DestinationPath ($output + '.zip')
        Write-Host "Package ready: $output.zip"
        Write-Host 'This script does not publish, deploy, sign, or overwrite an existing running EXE.'
    } finally { $lock.Dispose() }
} finally { Pop-Location }
