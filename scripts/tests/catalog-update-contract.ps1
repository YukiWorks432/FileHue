#requires -Version 7.0
param([string]$OutputDirectory)
$ErrorActionPreference = 'Stop'
. (Join-Path (Split-Path -Parent $PSScriptRoot) 'catalog-files.ps1')
function Payload([string]$License) {
    @{ commit = ('a' * 40); license_sha256 = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($License))).ToLowerInvariant(); languages = @(@{ id = 'rust'; extensions = @('.rs') }) } | ConvertTo-Json -Depth 8
}
$first = "Fixture permission 原文`n"
Set-CatalogFiles -TargetDirectory $OutputDirectory -Json (Payload $first) -License $first
$jsonPath = Join-Path $OutputDirectory 'languages.json'
$licensePath = Join-Path $OutputDirectory 'VSCODE-LICENSE.txt'
$beforeJson = [Convert]::ToBase64String([IO.File]::ReadAllBytes($jsonPath))
$beforeLicense = [Convert]::ToBase64String([IO.File]::ReadAllBytes($licensePath))
$locked = [IO.File]::Open($licensePath, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
$failed = $false
try {
    try { Set-CatalogFiles -TargetDirectory $OutputDirectory -Json (Payload 'Second permission') -License 'Second permission' } catch { $failed = $true }
} finally { $locked.Dispose() }
if (-not $failed) { throw '二番目の置換失敗を再現できません' }
if ([Convert]::ToBase64String([IO.File]::ReadAllBytes($jsonPath)) -cne $beforeJson -or [Convert]::ToBase64String([IO.File]::ReadAllBytes($licensePath)) -cne $beforeLicense) { throw '途中失敗で元の組み合わせが変わりました' }
# 前回の強制終了に相当する記録から、次回の実行時に復旧する。
$record = @{ format_version = 1; original = @{ 'languages.json' = @{ existed = $true; content = $beforeJson }; 'VSCODE-LICENSE.txt' = @{ existed = $true; content = $beforeLicense } } }
[IO.File]::WriteAllText((Join-Path $OutputDirectory '.catalog-update.pending.json'), ($record | ConvertTo-Json -Depth 8))
[IO.File]::WriteAllText($jsonPath, 'partial update')
Repair-CatalogFiles -TargetDirectory $OutputDirectory
if ([Convert]::ToBase64String([IO.File]::ReadAllBytes($jsonPath)) -cne $beforeJson) { throw '中断後の復旧に失敗しました' }
$failed = $false
try { Set-CatalogFiles -TargetDirectory $OutputDirectory -Json (Payload 'Different permission') -License $first } catch { $failed = $true }
if (-not $failed -or [Convert]::ToBase64String([IO.File]::ReadAllBytes($jsonPath)) -cne $beforeJson) { throw '整合しない組み合わせを保存しました' }
Write-Output '言語対応表: 通常更新・途中失敗・中断復旧・組み合わせ検証に成功'

# 元が空ファイルの場合も、削除せず空のバイト列を復元する。
$empty = Join-Path $OutputDirectory 'empty'
[IO.Directory]::CreateDirectory($empty) | Out-Null
[IO.File]::WriteAllBytes((Join-Path $empty 'languages.json'), [byte[]]::new(0))
[IO.File]::WriteAllBytes((Join-Path $empty 'VSCODE-LICENSE.txt'), [byte[]]::new(0))
$locked = [IO.File]::Open((Join-Path $empty 'VSCODE-LICENSE.txt'), [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
try { try { Set-CatalogFiles -TargetDirectory $empty -Json (Payload $first) -License $first } catch { } } finally { $locked.Dispose() }
foreach ($name in @('languages.json', 'VSCODE-LICENSE.txt')) {
    $file = Join-Path $empty $name
    if (-not [IO.File]::Exists($file) -or ([IO.FileInfo]$file).Length -ne 0) { throw '元の空ファイルを保持できませんでした' }
}
