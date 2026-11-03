<#
.SYNOPSIS
    xtask/tests/process.rsから起動する、プロセス制御試験専用の子プロセス。
.DESCRIPTION
    出力・異常終了・ファイルロック・子孫の残留を意図的に作る。
    待機中のプロセスとその子孫は、Rust側の試験が所有するWindowsジョブで回収する。
.PARAMETER Mode
    output: 大量出力、failure: 異常終了、holder: ロック保持、
    timeout: 子孫を起動して待機、linger: 子孫を残して正常終了。
.PARAMETER Value
    outputでは受け渡しを確かめる文字列。
    holder・timeout・lingerではロック対象ファイルのパス。failureでは使わない。
.PARAMETER Ready
    holder・timeout・lingerで、ロック取得を親へ知らせる合図用ファイルのパス。
#>
param(
    [string]$Mode,
    [string]$Value,
    [string]$Ready
)

$ErrorActionPreference = 'Stop'

if ($Mode -eq 'output') {
    # 引数の引用・Unicode・環境変数の設定と削除が正しく伝わるかを確かめる。
    $text = "$($env:WFI_FIXTURE_TEXT)|$Value|$($env:WFI_FIXTURE_REMOVE)"
    $stdout = [Console]::OpenStandardOutput()
    $stderr = [Console]::OpenStandardError()
    $stdout.Write([Text.Encoding]::UTF8.GetBytes($text))

    # 各200,000バイトを出し、出力量が多くても停止せず両方のログを保存できるかを試す。
    $stdout.Write([Text.Encoding]::ASCII.GetBytes(('X' * 200000)))
    $stderr.Write([Text.Encoding]::ASCII.GetBytes(('Y' * 200000)))
    return
}

if ($Mode -eq 'failure') {
    # Rust側は、この目印の保存と終了コード23の伝達を確認する。
    [Console]::Write('failure-marker')
    exit 23
}

if ($Mode -eq 'holder') {
    # 共有を許可せず開く。Rust側は子孫の回収後にこのロックが解除されるかを調べる。
    $lockedFile = [IO.File]::Open(
        $Value,
        [IO.FileMode]::OpenOrCreate,
        [IO.FileAccess]::ReadWrite,
        [IO.FileShare]::None
    )
    try {
        # 内容の確認用に文字xを書き、ロックを取得してから親へ合図する。
        $lockedFile.WriteByte([byte][char]'x')
        $lockedFile.Flush()
        [IO.File]::WriteAllText($Ready, 'ready')
        Start-Sleep -Seconds 60
    }
    finally {
        # 通常終了時の後始末。強制終了時はOSによるハンドル解放を試験する。
        $lockedFile.Dispose()
    }
    return
}

if ($Mode -notin @('timeout', 'linger')) {
    throw 'unknown fixture'
}

# 両モードとも、このスクリプトをholderとして再起動し、ロックを持つ子孫を作る。
# ArgumentListを使い、空白を含むパスもシェルのコマンド文字列へ埋め込まず渡す。
$startInfo = [Diagnostics.ProcessStartInfo]::new('pwsh')
$startInfo.UseShellExecute = $false
$startInfo.CreateNoWindow = $true
foreach ($argument in @('-NoProfile', '-File', $PSCommandPath, 'holder', $Value, $Ready)) {
    $startInfo.ArgumentList.Add($argument)
}
$childProcess = [Diagnostics.Process]::Start($startInfo)

# ロック取得前に親が終了しないよう合図を待つ。試験全体の待機期限はRust側が管理する。
while (-not [IO.File]::Exists($Ready)) {
    Start-Sleep -Milliseconds 10
}

# 強制終了後でも、終了直前までの標準出力・標準エラーを読めるかを確かめる。
[Console]::OpenStandardOutput().Write([Text.Encoding]::UTF8.GetBytes('準備完了'))
[Console]::Error.Write('partial-error')

if ($Mode -eq 'timeout') {
    # 自身も待機し、Rust側の期限超過で子孫まで回収されることを試す。
    Start-Sleep -Seconds 60
}
# lingerでは自身だけ正常終了し、残ったholderも回収されることを試す。
