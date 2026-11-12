#requires -Version 7.0
param(
    [Parameter(Mandatory)][string]$OutputPath,
    [Parameter(Mandatory)][string[]]$InputPaths,
    [ValidateRange(1, 100)][int]$Passes = 2
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$probe = Join-Path $root 'target/release/examples/provider_probe.exe'
$dll = Join-Path $root 'target/release/file_icons_thumbnail_shell.dll'
$destination = [IO.Path]::GetFullPath($OutputPath)
if ([IO.File]::Exists($destination)) { throw '出力先は未作成のファイルを指定してください。' }
if (-not [IO.File]::Exists($probe) -or -not [IO.File]::Exists($dll)) {
    throw '先に最適化したサムネイルDLLと計測プログラムをビルドしてください。'
}
$inputs = @($InputPaths | ForEach-Object {
    $file = Get-Item -LiteralPath $_
    if ($file.PSIsContainer -or $file.Extension.ToLowerInvariant() -notin @('.ai', '.eps', '.pdf', '.psb', '.psd', '.svg')) {
        throw "対応する形式のファイルを指定してください: $_"
    }
    $file
})
$observations = @(
    foreach ($file in $inputs) {
        for ($pass = 1; $pass -le $Passes; $pass++) {
            $start = [Diagnostics.ProcessStartInfo]::new($probe)
            $start.UseShellExecute = $false
            $start.CreateNoWindow = $true
            $start.RedirectStandardOutput = $true
            $start.RedirectStandardError = $true
            $start.ArgumentList.Add($dll)
            $start.ArgumentList.Add($file.FullName)
            $process = [Diagnostics.Process]::Start($start)
            try {
                if (-not $process.WaitForExit(20000)) {
                    $process.Kill($true)
                    $process.WaitForExit()
                    throw "計測が20秒以内に終わりませんでした: $($file.FullName)"
                }
                $stdout = $process.StandardOutput.ReadToEnd().Trim()
                $stderr = $process.StandardError.ReadToEnd().Trim()
                if ($process.ExitCode -ne 0 -or $stdout -notmatch '^outcome=(preview|unavailable) elapsed_us=(\d+) peak_working_set_bytes=(\d+)$') {
                    throw "計測プロセスが失敗しました: $($file.FullName) $stderr"
                }
                [pscustomobject]@{
                    path = $file.FullName
                    extension = $file.Extension.ToLowerInvariant()
                    size_bytes = $file.Length
                    pass = $pass
                    outcome = $Matches[1]
                    elapsed_us = [long]$Matches[2]
                    peak_working_set_bytes = [long]$Matches[3]
                }
            } finally {
                $process.Dispose()
            }
        }
    }
)
$groups = @($observations | Group-Object extension, outcome | ForEach-Object {
    $times = @($_.Group.elapsed_us | Sort-Object)
    $middle = [int][Math]::Floor($times.Count / 2)
    $median = if ($times.Count % 2) { $times[$middle] } else { ($times[$middle - 1] + $times[$middle]) / 2 }
    [pscustomobject]@{
        extension = $_.Group[0].extension
        outcome = $_.Group[0].outcome
        attempts = $times.Count
        median_us = $median
        p95_us = $times[[int][Math]::Ceiling($times.Count * 0.95) - 1]
        max_peak_working_set_bytes = ($_.Group.peak_working_set_bytes | Measure-Object -Maximum).Maximum
    }
})
$report = [pscustomobject]@{
    revision = (& git -C $root rev-parse HEAD).Trim()
    working_tree_dirty = [bool](& git -C $root status --porcelain)
    dll_sha256 = (Get-FileHash -LiteralPath $dll -Algorithm SHA256).Hash.ToLowerInvariant()
    passes = $Passes
    summary = $groups
    observations = $observations
}
$json = $report | ConvertTo-Json -Depth 6
$stream = [IO.File]::Open($destination, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write)
try {
    $writer = [IO.StreamWriter]::new($stream, [Text.UTF8Encoding]::new($false))
    try { $writer.WriteLine($json) } finally { $writer.Dispose() }
} finally { $stream.Dispose() }
Write-Output "計測結果: $destination"
