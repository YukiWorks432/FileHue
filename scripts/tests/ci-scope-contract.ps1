#requires -Version 7.4
$ErrorActionPreference = 'Stop'
$repository = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$classifier = Join-Path $repository 'scripts/ci-scope.ps1'
$fixture = Join-Path $repository ("work/ci-scope-tests/" + [Guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($fixture)
$passed = 0

function Invoke-FixtureGit([string[]]$Arguments) {
    $result = & git -C $fixture @Arguments
    if ($LASTEXITCODE -ne 0) { throw "Git試験の準備に失敗しました: $Arguments" }
    return $result
}
function Commit {
    Invoke-FixtureGit @('add', '--all') | Out-Null
    Invoke-FixtureGit @('-c', 'user.name=CI scope test', '-c', 'user.email=ci-scope@example.invalid',
        '-c', 'commit.gpgsign=false', 'commit', '-qm', '試験用の変更') | Out-Null
    return Invoke-FixtureGit @('rev-parse', 'HEAD')
}
function Check([string]$Name, [string]$Expected, [string]$Base, [string]$Head, [string]$EventName = 'push') {
    $actual = & $classifier -Repository $fixture -EventName $EventName -Base $Base -Head $Head
    if ($actual -cne $Expected) { throw "$Name : 期待=$Expected / 実際=$actual" }
    $script:passed++
    Write-Output "成功: $Name"
}
function Write-Fixture([string]$Path, [string]$Text) {
    $destination = Join-Path $fixture $Path
    [void][IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($destination))
    [IO.File]::WriteAllText($destination, $Text)
}

Invoke-FixtureGit @('init', '-q') | Out-Null
Write-Fixture 'README.md' '# 初期状態'
Write-Fixture 'docs/development.ja.md' '# 開発手順'
Write-Fixture 'apps/example.rs' '// 製品コード'
Write-Fixture 'docs/usage.ja.md' '# 配布する案内'
$base = Commit

Write-Fixture 'README.md' '# 更新した案内'
Write-Fixture 'docs/development.ja.md' '# 更新した開発手順'
Write-Fixture '.github/FUNDING.yml' 'github: example'
$docs = Commit
Check '配布外の文書と支援設定だけなら軽量化する' 'docs' $base $docs
Check '手動実行は文書だけでも全検証する' 'full' $base $docs 'workflow_dispatch'
Check '変更がなければ全検証する' 'full' $docs $docs
Check '比較元がなければ全検証する' 'full' ('0' * 40) $docs
Check '比較元を取得できなければ全検証する' 'full' ('1' * 40) $docs

Write-Fixture 'apps/example.rs' '// 更新した製品コード'
$code = Commit
Check 'コードと文書の混在では全検証する' 'full' $base $code
Write-Fixture 'README.md' '# コード変更の後の文書更新'
$afterCode = Commit
Check 'pushは最後が文書でも途中のコード変更を含める' 'full' $docs $afterCode

Write-Fixture 'docs/usage.ja.md' '# 更新した配布案内'
$payload = Commit
Check '配布文書は全検証する' 'full' $code $payload
Write-Fixture 'docs/new-guide.md' '# 未分類の文書'
$unknown = Commit
Check '未知の文書は全検証する' 'full' $payload $unknown
Write-Fixture 'LICENSE' '試験用の権利表示'
$license = Commit
Check '権利表示は全検証する' 'full' $unknown $license
Write-Fixture '.github/workflows/windows.yml' 'name: changed'
$workflow = Commit
Check 'CI設定は全検証する' 'full' $license $workflow

Invoke-FixtureGit @('mv', 'apps/example.rs', 'CONTRIBUTING.md') | Out-Null
$renamed = Commit
Check 'コードから文書への改名でも全検証する' 'full' $workflow $renamed
Invoke-FixtureGit @('rm', 'docs/usage.ja.md') | Out-Null
$deleted = Commit
Check '配布文書の削除も全検証する' 'full' $renamed $deleted

# PR側の文書だけの差分へ、基点側の別のコード変更を混ぜない。
Invoke-FixtureGit @('checkout', '-q', '-b', 'proposal', $base) | Out-Null
Write-Fixture 'README.md' '# PRの文書変更'
$proposal = Commit
Check 'PRは分岐点からheadまでを判定する' 'docs' $code $proposal 'pull_request'
Invoke-FixtureGit @('mv', 'README.md', 'README.en.md') | Out-Null
$docsRenamed = Commit
Check '配布外文書同士の改名は軽量化できる' 'docs' $proposal $docsRenamed
Write-Output "CI変更範囲の契約試験: $passed 件成功"
