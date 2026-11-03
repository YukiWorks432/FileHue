#requires -Version 7.0
[CmdletBinding()]
param([string]$Commit = '6d62b1d5dace93a33b3a6d3eca1317bd5e15bcb1')
$ErrorActionPreference = 'Stop'
if ($Commit -notmatch '^[0-9a-f]{40}$') { throw '完全なコミットSHAが必要です' }
$repoRoot = Split-Path -Parent $PSScriptRoot
. (Join-Path $PSScriptRoot 'catalog-files.ps1')
$target = Join-Path $repoRoot 'crates/icon-core/data'
Repair-CatalogFiles -TargetDirectory $target
if (-not (Get-Command gh -ErrorAction SilentlyContinue)) { throw 'GitHub CLIが必要です' }
$tree = gh api "repos/microsoft/vscode/git/trees/$Commit`?recursive=1" | ConvertFrom-Json
if ($LASTEXITCODE -ne 0 -or $tree.truncated) { throw '公式ソースの一覧を取得できません' }
$paths = $tree.tree | Where-Object { $_.path -match '^extensions/[^/]+/package.json$' } | Select-Object -ExpandProperty path
$manifests = $paths | ForEach-Object -Parallel {
    $ErrorActionPreference = 'Stop'
    $path = $_
    $uri = "https://raw.githubusercontent.com/microsoft/vscode/$using:Commit/$path"
    $manifest = Invoke-RestMethod -Uri $uri
    $definitions = foreach ($language in $manifest.contributes.languages) {
        if ($language.id) {
            [pscustomobject]@{ id = $language.id; extensions = @($language.extensions | Where-Object { $_ }); filenames = @($language.filenames | Where-Object { $_ }); patterns = @($language.filenamePatterns | Where-Object { $_ }); source = $uri }
        }
    }
    [pscustomobject]@{ path = $path; languages = @($definitions) }
} -ThrottleLimit 6 -ErrorAction Stop
if (@($manifests).Count -ne @($paths).Count) { throw '一部の言語定義を取得できません' }
$definitions = $manifests | ForEach-Object { $_.languages }
$languages = foreach ($group in ($definitions | Group-Object id | Sort-Object Name)) {
    [ordered]@{
        id = $group.Name
        extensions = @($group.Group.extensions | Sort-Object -Unique)
        filenames = @($group.Group.filenames | Sort-Object -Unique)
        patterns = @($group.Group.patterns | Sort-Object -Unique)
        sources = @($group.Group.source | Sort-Object -Unique)
    }
}
# すべての通信と検証を終えてから、二ファイルを一つの更新として保存する。
$license = Invoke-RestMethod -Uri "https://raw.githubusercontent.com/microsoft/vscode/$Commit/LICENSE.txt" -TimeoutSec 30
$license = $license.Replace("`r`n", "`n")
$licenseHash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($license))).ToLowerInvariant()
$data = [ordered]@{ repository = 'https://github.com/microsoft/vscode'; commit = $Commit; license_sha256 = $licenseHash; languages = @($languages) }
$json = ($data | ConvertTo-Json -Depth 10).Replace("`r`n", "`n") + "`n"
Set-CatalogFiles -TargetDirectory $target -Json $json -License $license
Write-Output "取得した公式言語定義: $($languages.Count)"
