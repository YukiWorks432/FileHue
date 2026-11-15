#requires -Version 7.4
param(
    [Parameter(Mandatory)][ValidatePattern('^[0-9]+\.[0-9]+\.[0-9]+$')][string]$Version,
    [Parameter(Mandatory)][string]$OutputPath,
    [string]$Root = (Split-Path -Parent $PSScriptRoot)
)
$ErrorActionPreference = 'Stop'
$baseUri = [Uri]"https://github.com/YukiWorks432/FileHue/blob/v$Version/docs/releases/"
$notes = @(foreach ($language in @('ja', 'en')) {
    $path = Join-Path $Root "docs/releases/$Version.$language.md"
    $text = [IO.File]::ReadAllText($path).Replace("`r`n", "`n")
    # 同梱ノートのインラインリンクを、同じタグの文書へ向ける。
    [regex]::Replace($text, '\]\(([^)\r\n]+)\)', {
        param($match)
        $target = $match.Groups[1].Value
        if ($target.StartsWith('#') -or $target.StartsWith('//') -or [Uri]::IsWellFormedUriString($target, [UriKind]::Absolute)) {
            return $match.Value
        }
        return '](' + [Uri]::new($baseUri, $target).AbsoluteUri + ')'
    }).TrimEnd([char[]]@([char]13, [char]10))
})
$body = [string]::Join("`n`n---`n`n", $notes) + "`n"
$output = [IO.Path]::GetFullPath($OutputPath)
[IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($output)) | Out-Null
$file = [IO.File]::Open($output, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write)
try {
    $bytes = [Text.UTF8Encoding]::new($false).GetBytes($body)
    $file.Write($bytes, 0, $bytes.Length)
} finally { $file.Dispose() }
