#requires -Version 7.4
param(
    [string]$EventName,
    [string]$Base,
    [string]$Head,
    [string]$Repository = (Join-Path $PSScriptRoot '..')
)
$ErrorActionPreference = 'Stop'

# 配布・ビルド・試験に使わない既知のファイルだけを軽量化する。
# 配布入力を追加するときは、この一覧との重複も確認する。
$lightweightPaths = @(
    'README.md', 'README.en.md', 'AGENTS.md', 'CONTRIBUTING.md',
    '.github/FUNDING.yml', '.github/pull_request_template.md',
    'docs/README.md', 'docs/architecture.ja.md', 'docs/current-specification.ja.md',
    'docs/dependency-security.ja.md', 'docs/design-references.ja.md', 'docs/design.ja.md',
    'docs/development.ja.md', 'docs/distribution.ja.md', 'docs/explorer-acceptance.ja.md',
    'docs/performance.ja.md', 'docs/requirements.ja.md', 'docs/ui-design.ja.md',
    'docs/ci/public-release.ja.md'
)

if ($EventName -notin @('push', 'pull_request') -or
    $Base -cnotmatch '^[0-9a-f]{40}$' -or $Head -cnotmatch '^[0-9a-f]{40}$' -or
    $Base -eq ('0' * 40) -or $Head -eq ('0' * 40)) {
    return 'full'
}

try {
    # PRは分岐点以降、pushはbeforeからの全変更を対象にする。
    $range = if ($EventName -eq 'pull_request') { "$Base...$Head" } else { "$Base..$Head" }
    # 改名は削除と追加に分け、元の配布入力やコードの削除を見落とさない。
    $names = & git -C $Repository diff --name-only --no-renames -z $range -- 2>$null
    if ($LASTEXITCODE -ne 0) { return 'full' }
    $paths = @(([string]::Join("`n", @($names))).Split([char]0, [StringSplitOptions]::RemoveEmptyEntries))
    if ($paths.Count -eq 0) { return 'full' }
    foreach ($path in $paths) {
        if ($lightweightPaths -cnotcontains $path) { return 'full' }
    }
    return 'docs'
} catch {
    # 比較元が取得できない場合も、検査を省略しない。
    return 'full'
}
