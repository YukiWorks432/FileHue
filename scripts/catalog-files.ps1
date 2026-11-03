#requires -Version 7.0
# 既知の二ファイルだけを更新し、途中失敗と前回中断から復旧する。
function Write-CatalogBytes {
    param([string]$Path, [byte[]]$Bytes)
    $temporary = Join-Path ([IO.Path]::GetDirectoryName($Path)) ('.catalog-' + [Guid]::NewGuid().ToString('N') + '.tmp')
    try {
        [IO.File]::WriteAllBytes($temporary, $Bytes)
        $stream = [IO.File]::Open($temporary, [IO.FileMode]::Open, [IO.FileAccess]::ReadWrite, [IO.FileShare]::Read)
        try { $stream.Flush($true) } finally { $stream.Dispose() }
        [IO.File]::Move($temporary, $Path, $true)
    } finally {
        if ([IO.File]::Exists($temporary)) { [IO.File]::Delete($temporary) }
    }
}
function Restore-CatalogTransaction {
    param([string]$TargetDirectory)
    $journal = Join-Path $TargetDirectory '.catalog-update.pending.json'
    if (-not [IO.File]::Exists($journal)) { return }
    if (([IO.FileInfo]$journal).Length -gt 32MB) { throw '更新記録が上限を超えています' }
    $record = [IO.File]::ReadAllText($journal) | ConvertFrom-Json
    if ($record.format_version -ne 1) { throw '未対応の更新記録です' }
    $restore = @{}
    foreach ($name in @('languages.json', 'VSCODE-LICENSE.txt')) {
        $entry = $record.original.PSObject.Properties[$name].Value
        if ($null -eq $entry -or $entry.existed -isnot [bool]) { throw '更新記録が壊れています' }
        $restore[$name] = if ($entry.existed) { ,([Convert]::FromBase64String($entry.content)) } else { $null }
    }
    foreach ($name in @('languages.json', 'VSCODE-LICENSE.txt')) {
        $destination = Join-Path $TargetDirectory $name
        $bytes = $restore[$name]
        if ($null -eq $bytes) {
            if ([IO.File]::Exists($destination)) { [IO.File]::Delete($destination) }
        } else {
            $same = [IO.File]::Exists($destination) -and [Convert]::ToBase64String([IO.File]::ReadAllBytes($destination)) -ceq [Convert]::ToBase64String($bytes)
            if (-not $same) { Write-CatalogBytes -Path $destination -Bytes $bytes }
        }
    }
    [IO.File]::Delete($journal)
}
function Repair-CatalogFiles {
    param([string]$TargetDirectory)
    $mutex = [Threading.Mutex]::new($false, 'Local\WindowsFileIconsCatalogUpdate')
    $owned = $false
    try {
        try { $owned = $mutex.WaitOne(0) } catch [Threading.AbandonedMutexException] { $owned = $true }
        if (-not $owned) { throw '別の言語対応表更新が実行中です' }
        Restore-CatalogTransaction -TargetDirectory $TargetDirectory
    } finally { if ($owned) { $mutex.ReleaseMutex() }; $mutex.Dispose() }
}
function Set-CatalogFiles {
    param([string]$TargetDirectory, [string]$Json, [string]$License)
    $encoding = [Text.UTF8Encoding]::new($false)
    $Json = $Json.Replace("`r`n", "`n")
    $License = $License.Replace("`r`n", "`n")
    $data = $Json | ConvertFrom-Json
    $licenseBytes = $encoding.GetBytes($License)
    if ($licenseBytes.Length -gt 8MB -or $encoding.GetByteCount($Json) -gt 8MB) { throw '更新データが上限を超えています' }
    $hash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($licenseBytes)).ToLowerInvariant()
    if ($data.commit -notmatch '^[0-9a-f]{40}$' -or @($data.languages).Count -eq 0 -or [string]::IsNullOrWhiteSpace($License) -or $data.license_sha256 -cne $hash) { throw '言語対応表と許諾文の整合を確認できません' }
    $mutex = [Threading.Mutex]::new($false, 'Local\WindowsFileIconsCatalogUpdate')
    $owned = $false
    try {
        try { $owned = $mutex.WaitOne(0) } catch [Threading.AbandonedMutexException] { $owned = $true }
        if (-not $owned) { throw '別の言語対応表更新が実行中です' }
        [IO.Directory]::CreateDirectory($TargetDirectory) | Out-Null
        Restore-CatalogTransaction -TargetDirectory $TargetDirectory
        $original = [ordered]@{}
        foreach ($name in @('languages.json', 'VSCODE-LICENSE.txt')) {
            $destination = Join-Path $TargetDirectory $name
            $exists = [IO.File]::Exists($destination)
            if ($exists -and ([IO.FileInfo]$destination).Length -gt 8MB) { throw '元の対応表または許諾文が上限を超えています' }
            $original[$name] = @{ existed = $exists; content = if ($exists) { [Convert]::ToBase64String([IO.File]::ReadAllBytes($destination)) } else { '' } }
        }
        $journal = Join-Path $TargetDirectory '.catalog-update.pending.json'
        $record = @{ format_version = 1; original = $original } | ConvertTo-Json -Depth 8
        Write-CatalogBytes -Path $journal -Bytes ($encoding.GetBytes($record))
        try {
            Write-CatalogBytes -Path (Join-Path $TargetDirectory 'languages.json') -Bytes ($encoding.GetBytes($Json))
            Write-CatalogBytes -Path (Join-Path $TargetDirectory 'VSCODE-LICENSE.txt') -Bytes $licenseBytes
            [IO.File]::Delete($journal)
        } catch {
            $failure = $_
            Restore-CatalogTransaction -TargetDirectory $TargetDirectory
            throw $failure
        }
    } finally { if ($owned) { $mutex.ReleaseMutex() }; $mutex.Dispose() }
}
