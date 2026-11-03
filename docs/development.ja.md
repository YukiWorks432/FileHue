# 開発と検証

Windows 11 x64とRustのMSVCツールチェーンを使う。依存版の正本はCargo.lock、開発コマンドの正本はjustfileである。本書は現在の手順を扱い、過去の試験成功を現在のソースの検証結果として引き継がない。

## 構成

構成と責務は[アーキテクチャ](architecture.ja.md)を参照する。

## 検証コマンド

PowerShell 7.4以降、RustのMSVC環境、justを用意する。リポジトリのルートで `just --list` を実行するとコマンド一覧を確認できる。

依存監査にはcargo-deny 0.20.2を事前導入する。[依存の更新と監査](dependency-security.ja.md)に従い、更新候補をコンパイルする前に`just audit`を実行する。

| コマンド | 用途 |
| --- | --- |
| `just audit` | 固定版cargo-denyで、例外の期限・対象版と全体・Windowsの依存をビルド前に検査 |
| `just test-policy` | 監査成功後に、例外の拒否と設定の受け渡しをRustの契約試験で確認 |
| `just run` / `just run-debug` | リリース／デバッグ構成でGUIとDLLをビルドして起動 |
| `just cli --help` | 補助CLIのビルドとヘルプ |
| `just build` / `just build-release` | ワークスペースをビルド |
| `just fmt` / `just fmt-check` | Rustの整形／整形確認 |
| `just clippy` | GPUI試験構成を含む全対象を警告なしで静的解析 |
| `just test` | ビルド後にGPUI操作・隔離登録・契約試験を実行 |
| `just test-scripts` | 配布・プロセス制御・言語表更新のRust試験 |
| `just smoke` | 専用プロファイルでGUIの起動と終了を確認 |
| `just verify` | 整形確認、Clippy、ビルド・Rust試験、xtaskの契約試験、GUI起動確認 |
| `just package` | 変更のないソースから検証用ZIPを作成 |
| `just licenses [--output <新しいフォルダー>]` | cargo-aboutの選択結果・権利表示の原文・閲覧用HTMLを生成 |
| `just check-binaries --folder <フォルダー>` | 配布EXE・DLLのビルド元パスを検査 |
| `just perf-gui <未作成のJSON>` / `just perf-icons <未作成のJSON>` | GUI・関連付け調査／ICO変換の時間とメモリーを手動計測 |

個別のRustコマンドが必要な場合は、Shell結合試験より前にDLLをビルドする。

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --features file-icons-manager/test-support --locked -- -D warnings
cargo build --workspace --locked
cargo test --workspace --features file-icons-manager/test-support --locked
cargo test -p xtask --locked
pwsh -NoProfile -File scripts/smoke-gui.ps1
```


Shell試験は、実在するProgIDを共有する種類と、ProgIDが未登録の拡張子を分けて準備する。存在しないProgIDへの別名に依存せず、元アイコンの画素、パックの適用先、復元後の全登録を照合する。保護対象外のアイコン照会に失敗した場合は、実際の関連付け、同じDLLへの直接呼び出し、試験プロセスのトークンをログへ記録する。

同じPCで負荷の高い試験・性能測定を同時に実行しない。個人環境の共通CPU排他を使う場合はその手順に従う。ビルド単独は排他対象としない。

GUIの起動だけでは登録は変わらない。通常のGUI・CLI操作は実際の設定を使うため、開発PCへの適用には明示的な許可を得る。適用ではビルド出力のDLLを直接登録せず、ユーザー領域へコピーした内容ハッシュ別のDLLを使う。

## 開発専用のxtask

`xtask/` に配布・検証・プロセス制御を置き、`.cargo/config.toml` の別名から `cargo xtask` として呼び出す。通常はjustから実行する。

- `just package [--profile debug|release] [--allow-dirty]`: 検証用ZIP・展開内容・`SHA256SUMS.txt`を一つの出力フォルダーへ作成する。
- `just package --public-candidate`: 新しい公開先のクリーンなコミットから未署名の公開候補を作成する。GitHub CLIも必要とする。
- `cargo xtask package --verify <展開先> [--metadata <外部記録のフォルダー>] [--public-candidate]`: 指定した用途とZIP外のマニフェスト・権利確認記録を照合する。外部記録の既定は展開先の親フォルダー。
- `just verify-release --report <報告書> [--public-candidate]`: 作成から配布DLL・GUI・CLI・文書の確認まで行う。既存ZIPには `--archive <ZIP>` を指定し、隣接する`SHA256SUMS.txt`も保持する。公開候補の報告にはローカルパスと生ログの場所を含めない。
- `just check-docs`: 文書の相対リンクを照合する。
- `just test-tools` / `just test-scripts`: 開発補助のRust試験を実行する。

xtaskはワークスペース内の開発用プログラムであり、GUI・CLI・Shellの通常依存と配布対象には加えない。Rust試験から使う場合も開発専用依存として扱う。

xtask内から呼ぶ製品のビルド・試験ビルドは `--workspace --exclude xtask` とし、実行中の補助プログラム自身を書き換えない。xtask自体の試験は `just verify` または `just test-tools` で確認する。

## 操作・表示・実機確認の区別

GPUIのtest-supportはクリックとキー入力、画面遷移、下書き、保存、処理中の制限を検証する。描画画像では日英・明暗・狭い幅・文字拡大と、面・余白・選択の見え方を別に確認する。手順は[デザイン規約](ui-design.ja.md#変更時の確認)に従う。

隔離登録試験は専用の設定とレジストリを使い、通常のExplorer全体を操作する試験とは区別する。`just verify` は実Explorerでの受け入れや公式パックの実取得を含まない。[実機受け入れ](explorer-acceptance.ja.md)、[配布物の検証](distribution.ja.md)、[性能計測](performance.ja.md)は必要に応じて別に行う。結果には対象コミット、コマンド、環境、終了コードと未確認事項を記録し、ログと画像は `work/` に保存する。利用者のパスやレジストリ情報を含む記録をそのまま公開しない。


## 試験プロセスの待機期限と診断

Rustの隔離試験は `test-support/process.rs` から開発専用の `xtask::process` を呼び、同じWindowsジョブの所有境界で子孫を管理する。対象を中断状態で作成し、ジョブへの割り当て後に開始する。成功・失敗時とも残った所有子孫を回収し、同名の別プロセスやExplorerは終了しない。プロセス制御をアプリ本体へ組み込むものではない。

Rustの子試験は180秒、GUI起動確認は30秒で制限する。配布検証はビルド1800秒、STA/MTA全体420秒、CLI30秒を使う。終了回収は別に最大5秒待ち、回収失敗も検証失敗とする。長い公式取得を明示実行する場合も上限の存在を維持する。

標準出力と標準エラーはパイプを使わず専用ログへ書き込み、出力量による待ち合わせを避ける。開始と終了には試験名・PID・経過時間を記録する。Rust側の期限超過・制御失敗は `work/processes/rust-probe-*` を保持し、途中ログと実行内容の所在を案内する。試験キーの作成前にも専用HKCUキーをログへ出す。強制終了では子のDropが実行されないため、残留する試験キーやプロファイルはログを根拠に調べる。通常のユーザー設定を一括削除して復旧しない。

実プロセスによる回帰試験は、大量の標準出力・標準エラー、Unicodeの引数と環境、異常終了、期限超過、孫プロセスのロック解放、無関係なプロセスの生存を確認する。`cargo test -p xtask --locked` と `just verify` を他の負荷の高い試験と重ならないように実行する。

## 用途別カテゴリの保守

分類表と同梱言語定義から、テーマに依存しないカテゴリを組み立てる。分類の優先順位・用途の境界・未分類一覧の取得方法は[適用対象の選択](target-selection.ja.md#分類表の保守と未分類の確認)を参照する。言語定義を更新する際は、分類未確認の言語を共通処理の試験で検出する。分類表の重複・不明なカテゴリも検査する。

分類の契約試験は、SQL・Lua・Dartなどの言語、拡張子と固有名の区別、複合拡張子、同梱の名前パターン、曖昧な指定、パック間の一致、保存済みの適用可否を検証する。CLIの実プロセス試験では日英のカテゴリ出力と圧縮対象だけの一括無効化を確認する。期待値は用途表と具体例から定め、実装の表をそのまま期待値に転記しない。

カテゴリ欄はGPUI KitのListとListItemによる単一選択の一覧を使う。GPUIの対象編集試験では、上下キーだけでは絞り込みを変更しないこと、Enterとクリックによる確定、カテゴリ操作では保存しないこと、狭い画面でのEscと確定後の検索欄へのフォーカス復帰を確認する。描画確認用の `target_editor_visual_probe` は、日英・明暗・通常幅と狭い幅の対象編集、狭い幅のカテゴリ一覧、選択中とキーボード移動位置が異なる状態を計7枚に出力する。画像生成後に文字の収まり、行の境界、選択背景とフォーカス枠を確認する。

```powershell
cargo test -p file-icons-core --locked
cargo test -p file-icons-cli --test cli shared_categories_drive_bulk_edits_in_both_languages --locked
```

公開API、GUI・CLI、Shellへの影響も確認する変更では、上記に加えて既定の`just verify`を他の負荷の高い試験と重ならないように実行する。分類だけの更新は通常プロファイルへのShell登録・実Explorerでの再受け入れを伴わない。

## 依存の監査

通常更新の7日待機、ビルド前の検査、個別例外と期限、公開後の更新提案は[依存の更新と監査](dependency-security.ja.md)を正本とする。cargo-deny 0.20.2を事前導入し、最初に`just audit`を使う。`just verify`も監査と例外期限の試験から始める。候補依存でxtaskをコンパイルしてから監査する構成にはしない。

### 権利表示の生成

`cargo install cargo-about --version 0.9.2 --locked`で固定版を導入する。依存を取得済みの環境で`just licenses`を実行すると、一意な`work/licenses-*`フォルダーへ生成する。保存先は`--output`でも指定できるが、既存のフォルダーは上書きしない。配布ZIPも同じ生成処理を必須で実行する。異なるツール版、許可一覧にないライセンス、生成失敗、補完元のハッシュ不一致では成功成果物を作らない。

`just run`でアプリ情報画面からライセンスを開くには、先に`just build-release`を実行し、次をリポジトリのルートから実行する。`LICENSE`や`licenses`が出力先に既にある場合は上書きせず停止する。デバッグ構成では`release`を`debug`に置き換え、先に`just build`を実行する。

```powershell
if (Test-Path target/release/LICENSE) { throw 'target/release/LICENSE already exists' }
Copy-Item -LiteralPath LICENSE -Destination target/release/LICENSE
just licenses --output target/release/licenses
```

画面は作業ディレクトリに依存せず、実行ファイルの隣の`LICENSE`と`licenses/THIRD_PARTY_LICENSES.html`を探す。未準備や欠落時は画面に案内を表示し、権利表示の自動生成や取得は行わない。

`about.toml`を条件選択の正本とし、Apache-2.0を優先する。対象はWindows x64・既定の機能・通常依存とビルド依存であり、開発専用依存を除外する。ワークスペース全体で採取した結果をGUI・CLI・Shell DLLの依存IDへ照合し、開発補助専用の部品とローカル絶対パスを出力から除く。従来の保守的な原文収集には、通常ビルドで使わない機能由来の依存が残る場合がある。これらは`selection_status=supplement-only`とし、cargo-aboutが選択したものと区別する。

`licenses/THIRD_PARTY_LICENSES.html`（単独生成では保存先直下の`THIRD_PARTY_LICENSES.html`）にライセンス全文と原文を掲載し、提供ソースへリンクする。機械可読な選択結果は`about.json`、原文の名前・ハッシュ・補完元・個別の未確認事項は`dependencies.json`に記録する。cargo-aboutが標準本文を補う場合もあるため、その生成結果だけで原文や著作権表示の不足を解消したと扱わない。詳細は[配布手順](distribution.ja.md#権利表示の収集範囲)を参照する。

生成・検証処理を変更した場合は、`cargo test -p xtask --locked`、`cargo clippy -p xtask --all-targets --locked -- -D warnings`、`just licenses`、`just check-docs`で確認する。補完の改変拒否、原文の保持、依存IDと条件選択の一致、複数条件の保持、HTMLのエスケープを契約として検査する。GUIを変更しないため、この範囲ではGPUI操作試験やExplorer再登録は不要である。

### 脆弱性と依存条件の確認

依存を更新するとき、上流の新しいアドバイザリーを確認したとき、公開候補を確定する前に監査する。Cargo.lockを固定し、使用したcargo-denyの版、対象コミット、対象OS・機能、アドバイザリーデータベースのコミットと取得時点、各コマンドの終了コードを記録する。データベースの保存先を確認し、そのチェックアウトで `git rev-parse HEAD` を実行して版を特定する。データベースの取得に失敗した場合や古いデータだけで実行した場合は、その制約も明示する。

リポジトリのルートで次を実行する。`just audit`は例外の期限と対象版を照合したうえで、OSを絞らない検査とWindows配布対象の検査を順に行う。Windowsだけの成功で他の対象を確認済みにしない。

```powershell
git rev-parse HEAD
cargo deny --version
just audit
```

終了コードと診断を併せて確認し、脆弱性、健全性の問題、保守停止、取り下げられた版を区別する。脆弱性の件数、特定の指摘の解消、監査コマンド全体の成功は別の結果として扱う。ライブラリの指摘対象APIを使っていない場合も、依存の問題を消えたことにせず、呼び出し経路と既存の防御を根拠に影響を記録する。

保守停止の依存は、アドバイザリーID、名前と版、直接の親から本製品までの依存経路、影響するOSと機能、上流の対応状況、追跡日、見直す条件を一件ずつ確認する。経路は `cargo tree --locked --target all --invert 名前@版` と、`--target x86_64-pc-windows-msvc` を指定した結果を照合する。名前と版は診断に出た実際の値に置き換える。上流の対応版が出た場合、依存経路が変わった場合、新たな脆弱性が報告された場合に再評価する。保守停止だけを理由に、GUIや画像変換の依存一式を無検証で更新しない。

監査の例外が必要な場合は個別IDに限り、理由、依存経路、影響範囲、追跡日、見直す条件を記録する。保守停止全体の無視や推移的依存の検査除外で隠さず、未使用になった例外も検出する。

ライセンスの選択は`about.toml`とcargo-aboutで管理し、配布に必要な原文・通知・提供ソースも照合する。`just audit`の依存監査と、`just licenses`による権利表示の生成は両方確認する。設定が存在しない状態の `cargo deny --locked check licenses` は既定の拒否で失敗し得るため、その結果だけで違反とも配布可能とも判断しない。

## Windows向けCI

Windows検証では監査後にShellの隔離試験を先に実行し、GPUIを含む全体ビルドより前に登録経路を確認する。拡張子自身の保護と別の種類への適用は、登録前のキャッシュを持たない別の隔離プロセスからも実際のShell APIで照合する。固定版ツールはソースの外で導入し、導入成功時点でツールの保存先だけをキャッシュする。以降の試験に失敗した場合も、再実行で同じツールを再構築せずに復元できる。キャッシュの識別子にはRustと各ツールの版を含める。

[Windows検証](../.github/workflows/windows.yml)はPRとmainへの変更、および手動実行で動く。固定版のツールとActionを使い、`just verify`、文書のリンク、測定結果の集計、最適化ZIPと同梱DLLを検証する。公式パックの実取得は手動実行の`verify_downloads`で選ぶ。検証の成功は実Explorerの受け入れやリリース公開を意味しない。

[依存監査](../.github/workflows/dependency-audit.yml)は毎日と手動で実行し、製品コードをビルドしない。配布候補は[専用ワークフロー](../.github/workflows/public-release.yml)をmainから手動実行し、[配布候補の生成と公開](ci/public-release.ja.md)に従う。
