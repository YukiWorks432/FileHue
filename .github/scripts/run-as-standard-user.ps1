# GitHub管理の使い捨てVMだけで、ユーザー単位COM登録を有効にして検証する。
[CmdletBinding()]
param([Parameter(Mandatory)][string]$Command)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted') {
    throw 'このスクリプトはGitHub管理のランナー専用です。'
}
$workspace = (Resolve-Path -LiteralPath $env:GITHUB_WORKSPACE).ProviderPath
$runtime = Join-Path $env:RUNNER_TEMP ('filehue-user-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $runtime | Out-Null
$cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE '.cargo' }
$rustupHome = if ($env:RUSTUP_HOME) { $env:RUSTUP_HOME } else { Join-Path $env:USERPROFILE '.rustup' }
$name = 'fh-' + [guid]::NewGuid().ToString('N').Substring(0, 12)
$password = 'Fh1!' + [Convert]::ToBase64String([Security.Cryptography.RandomNumberGenerator]::GetBytes(32))
$secure = ConvertTo-SecureString $password -AsPlainText -Force
$user = New-LocalUser -Name $name -Password $secure -AccountNeverExpires
try {
    Add-LocalGroupMember -SID 'S-1-5-32-545' -Member $user
    # ソース・ビルド先とツールの保存先だけに権限を渡す。管理者グループには追加しない。
    foreach ($path in @($workspace, $runtime, $cargoHome)) {
        & icacls.exe $path /grant ("*" + $user.SID.Value + ':(OI)(CI)M') | Out-Null
        if ($LASTEXITCODE -ne 0) { throw '検証領域の権限を設定できませんでした。' }
    }
    & icacls.exe $rustupHome /grant ("*" + $user.SID.Value + ':(OI)(CI)RX') | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'Rustの読取権限を設定できませんでした。' }
    $childScript = @'
$ErrorActionPreference = 'Stop'
try {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = [Security.Principal.WindowsPrincipal]::new($identity)
    if ($principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        throw '標準ユーザーとして起動されていません。'
    }
    Write-Host '標準ユーザーで検証を実行します。'
    Set-Location -LiteralPath $env:GITHUB_WORKSPACE
    & ([scriptblock]::Create($env:FILEHUE_CI_COMMAND))
    exit $LASTEXITCODE
} catch {
    Write-Error $_
    exit 1
}
'@
    $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($childScript))
    $environment = @{
        CARGO_HOME = $cargoHome
        RUSTUP_HOME = $rustupHome
        TEMP = $runtime
        TMP = $runtime
        LOCALAPPDATA = (Join-Path $runtime 'localappdata')
        FILEHUE_CI_COMMAND = $Command
        # チェックアウトの所有者はランナーのまま。恒久的なGit設定は変更しない。
        GIT_CONFIG_COUNT = '1'
        GIT_CONFIG_KEY_0 = 'safe.directory'
        GIT_CONFIG_VALUE_0 = $workspace
    }
    $credential = [pscredential]::new("$env:COMPUTERNAME\$name", $secure)
    $process = Start-Process -FilePath (Get-Command pwsh.exe).Source -Credential $credential -LoadUserProfile -WindowStyle Hidden -WorkingDirectory $workspace -ArgumentList @('-NoProfile', '-NonInteractive', '-EncodedCommand', $encoded) -Environment $environment -RedirectStandardOutput (Join-Path $runtime 'stdout.log') -RedirectStandardError (Join-Path $runtime 'stderr.log') -Wait -PassThru
    Get-Content -LiteralPath (Join-Path $runtime 'stdout.log')
    Get-Content -LiteralPath (Join-Path $runtime 'stderr.log')
    if ($process.ExitCode -ne 0) { throw "標準ユーザーでの検証に失敗しました（終了コード $($process.ExitCode)）。" }
} finally {
    Remove-LocalUser -SID $user.SID
}
