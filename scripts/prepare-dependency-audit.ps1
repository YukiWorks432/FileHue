#requires -Version 7.4
param(
    [string]$ExceptionsPath = (Join-Path $PSScriptRoot '../security/advisory-exceptions.json'),
    [string]$ConfigPath = (Join-Path $PSScriptRoot '../deny.toml'),
    # 契約試験ではコンパイル不要のmetadata取得も固定入力へ置き換える。
    [string]$MetadataPath,
    [string]$OutputPath = (Join-Path $PSScriptRoot ("../work/dependency-audit/deny-$([Guid]::NewGuid().ToString('N')).toml"))
)
$ErrorActionPreference = 'Stop'
$exceptions = Get-Content -LiteralPath $ExceptionsPath -Raw | ConvertFrom-Json -NoEnumerate
if ($exceptions -isnot [array]) { throw '監査例外は配列で指定してください。' }
$ids = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
foreach ($exception in $exceptions) {
    if ($exception.id -cnotmatch '^RUSTSEC-\d{4}-\d{4}$' -or -not $ids.Add($exception.id) -or
        [string]::IsNullOrWhiteSpace($exception.reason) -or [string]::IsNullOrWhiteSpace($exception.path) -or
        $exception.package -notmatch '^[A-Za-z0-9_-]+@[^@\s]+$') {
        throw '監査例外が不正または重複しています。'
    }
    $deadline = [DateTime]::ParseExact($exception.review_by, 'yyyy-MM-dd', [Globalization.CultureInfo]::InvariantCulture)
    if ([DateTime]::UtcNow.Date -ge $deadline) {
        throw "監査例外を再確認してください: $($exception.id) / $($exception.review_by) UTC"
    }
}
if ($MetadataPath) {
    $metadata = Get-Content -LiteralPath $MetadataPath -Raw | ConvertFrom-Json
} else {
    # metadataは依存を取得し得るが、ビルドスクリプトや手続きマクロを実行しない。
    Push-Location (Join-Path $PSScriptRoot '..')
    try {
        $text = & cargo metadata --locked --format-version 1
        if ($LASTEXITCODE -ne 0) { throw '依存の対象版を取得できませんでした。' }
        $metadata = $text | ConvertFrom-Json
    } finally { Pop-Location }
}
foreach ($group in ($exceptions | Group-Object { $_.package.Split('@')[0] })) {
    $approved = [Collections.Generic.HashSet[string]]::new([string[]]$group.Group.package, [StringComparer]::Ordinal)
    $present = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    foreach ($package in $metadata.packages) {
        if ($package.name -eq $group.Name) { [void]$present.Add($package.name + '@' + $package.version) }
    }
    if (-not $approved.SetEquals($present)) {
        throw "例外の対象版を再確認してください: $($group.Name) / 承認: $($approved -join ', ') / 検出: $($present -join ', ')"
    }
}
# IDと理由を引用し、理由の引用符や改行もTOMLへの命令として解釈させない。
$config = Get-Content -LiteralPath $ConfigPath -Raw
foreach ($exception in $exceptions) {
    $config += "`n[[advisories.ignore]]`nid = $($exception.id | ConvertTo-Json -Compress)`nreason = $($exception.reason | ConvertTo-Json -Compress)`n"
}
$output = [IO.Path]::GetFullPath($OutputPath)
[IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($output)) | Out-Null
# 前回成功した設定や利用者のファイルを上書きしない。
$file = [IO.File]::Open($output, [IO.FileMode]::CreateNew)
try {
    $bytes = [Text.UTF8Encoding]::new($false).GetBytes($config)
    $file.Write($bytes)
} finally { $file.Dispose() }
Write-Output $output
