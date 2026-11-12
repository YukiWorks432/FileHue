# PowerShell 7.4以降。引数をコマンド文字列へ埋め込まず、そのまま渡す。
set shell := ["pwsh", "-NoLogo", "-NoProfile", "-CommandWithArgs"]
set positional-arguments

# PowerShellの$args[0]に渡されるレシピ名は、転送時に除外する。

# 基本コマンドの一覧
default:
    @just --list

# 候補依存をコンパイルせず、固定版cargo-denyで全体とWindows対象を監査する
audit:
    $ErrorActionPreference = 'Stop'; \
    if ((& cargo deny --version) -ne 'cargo-deny 0.20.2' -or $LASTEXITCODE -ne 0) { throw 'cargo-deny 0.20.2を事前に導入してください。' }; \
    $config = & ./scripts/prepare-dependency-audit.ps1; \
    cargo deny --locked --config $config check --deny advisory-not-detected advisories bans sources; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }; \
    cargo deny --locked --config $config --target x86_64-pc-windows-msvc check advisories bans sources; exit $LASTEXITCODE

# 監査前の例外期限・不一致の拒否を検証する
test-policy:
    cargo test -p xtask --test audit_policy --locked

# 実Git差分で、軽量検証の選択と全検証への退避を確認する
test-ci-scope:
    & ./scripts/tests/ci-scope-contract.ps1

# 最適化したGUIを起動する（Shell DLLも同じ構成でビルドする）
run *args: build-release
    $forward = @($args | Select-Object -Skip 1); & ./target/release/file-icons-manager.exe @forward; exit $LASTEXITCODE

# デバッグ用GUIを起動する（Shell DLLも先にビルドする）
run-debug *args: build
    $forward = @($args | Select-Object -Skip 1); & ./target/debug/file-icons-manager.exe @forward; exit $LASTEXITCODE

# 補助CLIを実行する（例: just cli --help）
cli *args: build
    $forward = @($args | Select-Object -Skip 1); & ./target/debug/file-icons.exe @forward; exit $LASTEXITCODE

# 開発用のGUI・CLI・Shell DLLをビルドする
build:
    cargo build --workspace --locked

# 最適化したGUI・CLI・Shell DLLをビルドする
build-release:
    cargo build --workspace --release --locked

# Rustコードを整形する
fmt:
    cargo fmt --all

# Rustコードの整形を検査する
fmt-check:
    cargo fmt --all -- --check

# GPUI試験用の構成を含めて静的解析する
clippy:
    cargo clippy --workspace --all-targets --features file-icons-manager/test-support --locked -- -D warnings

# 既存のモック・契約試験とGPUI操作試験を実行する
test: build
    cargo test --workspace --features file-icons-manager/test-support --locked

# サムネイル資源の抽出と描画の契約を確認する
test-thumbnails:
    cargo build -p file-icons-thumbnail-shell --locked
    cargo test -p file-icons-thumbnail-core -p file-icons-thumbnail-shell --locked

# 指定したファイルのサムネイル取得を登録せずに確認する
probe-thumbnails input:
    cargo build -p file-icons-thumbnail-shell --lib --example provider_probe --locked; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }; & ./target/debug/examples/provider_probe.exe ([IO.Path]::GetFullPath('target/debug/file_icons_thumbnail_shell.dll')) ([IO.Path]::GetFullPath($args[1])); exit $LASTEXITCODE

# 準備済みの最適化ビルドで明示したファイルを計測する
perf-thumbnails output *inputs:
    $forward = @($args | Select-Object -Skip 2); & ./scripts/measure-thumbnails.ps1 -OutputPath $args[1] -InputPaths $forward

# Windows 11の通常権限で、実DLLとShellの隔離結合試験を実行する
test-shell:
    cargo build -p file-icons-shell -p file-icons-thumbnail-shell --locked
    cargo test -p file-icons-shell --features shell-integration --test isolated_shell --locked -- --nocapture
    cargo test -p file-icons-windows --lib thumbnails::inventory::windows::tests::reads_only_redirected_registry --locked -- --nocapture
    cargo test -p file-icons-thumbnail-shell --test pdf --locked -- --nocapture

# 配布・プロセス制御・更新処理のRust試験を実行する
test-tools:
    cargo test -p xtask --locked

# 開発補助のRust試験を実行する（test-toolsと同じ）
test-scripts: test-tools

# 専用プロファイルでGUIの起動・正常終了を確認する
smoke: build
    & ./scripts/smoke-gui.ps1

# 日常の一括検証（実Shell・Explorer受け入れと外部パックの実取得は別途実施）
verify: audit test-policy fmt-check clippy test test-scripts smoke

# 検証用ZIPを作成する（公開先からは--public-candidateで公開候補を指定）
package *args:
    $forward = @($args | Select-Object -Skip 1); cargo xtask package @forward; exit $LASTEXITCODE

# 同じ完成ZIPを検証する（CIは--skip-shell、Windows 11での最終確認は既定で実Shellも実行）
verify-release *args:
    $forward = @($args | Select-Object -Skip 1); cargo xtask verify-release @forward; exit $LASTEXITCODE

# 文書の相対リンクを検査する
check-docs:
    cargo xtask check-docs

# 専用プロファイルでGUI・関連付けの時間と割り当て量を計測する（出力は上書きしない）
perf-gui output:
    cargo build -p file-icons-cli --release --locked; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }; $env:WFI_PERF_CLI = [IO.Path]::GetFullPath('target/release/file-icons.exe'); $env:WFI_PERF_STARTUP = '1'; $env:WFI_PERF_OUTPUT = [IO.Path]::GetFullPath($args[1]); $env:WFI_PERF_REVISION = git rev-parse HEAD; cargo test -p file-icons-manager --release --features test-support --locked ui_tests::performance::gui_performance -- --ignored --exact --nocapture; exit $LASTEXITCODE

# ICO変換の時間・ピーク割り当て量と出力の同一性を計測する
perf-icons output:
    $env:WFI_PERF_OUTPUT = [IO.Path]::GetFullPath($args[1]); $env:WFI_PERF_REVISION = git rev-parse HEAD; cargo test -p file-icons-core --release --locked theme::performance::ico_performance -- --ignored --exact --nocapture; exit $LASTEXITCODE

# 権利表示の原文・cargo-aboutの選択結果・閲覧用HTMLを生成する
licenses *args:
    $forward = @($args | Select-Object -Skip 1); cargo xtask licenses @forward; exit $LASTEXITCODE

# EXE・DLLにビルド元の個人パスが残っていないか検査する
check-binaries *args:
    $forward = @($args | Select-Object -Skip 1); cargo xtask check-binaries @forward; exit $LASTEXITCODE
