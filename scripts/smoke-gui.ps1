#requires -Version 7.0
param(
    [string]$ExecutablePath,
    [string]$ProfilePath
)
$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
$executable = if ($ExecutablePath) { [System.IO.Path]::GetFullPath($ExecutablePath) } else { Join-Path $repoRoot 'target/debug/file-icons-manager.exe' }
$profileRoot = if ($ProfilePath) { [System.IO.Path]::GetFullPath($ProfilePath) } else { Join-Path $repoRoot 'work/gui-smoke/profile' }
[System.IO.Directory]::CreateDirectory($profileRoot) | Out-Null
$logRoot = Split-Path -Parent $profileRoot
& cargo xtask smoke --executable $executable --profile $profileRoot
if ($LASTEXITCODE -ne 0) { throw "GUI起動確認に失敗しました。ログ: $logRoot" }
Write-Output 'GUI起動・ウィンドウ初期化・自動終了: 成功（描画と操作の受け入れは別途必要）'
Get-FileHash -LiteralPath $executable -Algorithm SHA256 | Select-Object Path,Hash
