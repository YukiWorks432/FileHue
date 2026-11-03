# FileHue

[English](README.en.md) | 日本語

Windows 11 x64の標準エクスプローラーで、VS Codeのファイルアイコンテーマを使うRust製アプリです。ファイルの内容や既定のアプリを変更せず、ファイル名と拡張子からアイコンを選びます。

0.1.0のWindows x64向けZIPは準備中です。[GitHub Releases](https://github.com/YukiWorks432/FileHue/releases)から公開予定で、初回は未署名で配布します。公開日は未確定です。0.1.0の機能と制限は[初回リリースの案内](docs/releases/0.1.0.ja.md)、配布前の確認は[配布手順](docs/distribution.ja.md)を参照します。

## できること

- GUIから確認済みのアイコンパックを取得し、プレビューと利用条件を見てから適用します。
- vscode-icons、Material Icon Theme、Catppuccin Icons、Symbols、VSCode Great Iconsを選びます。手元の展開済みVS Code拡張も取り込めます。
- テーマに定義があれば、Cargo.tomlと通常のTOML、package.jsonと通常のJSON、README.mdと通常のMarkdownを区別します。新規作成や改名にも対応します。
- 適用するファイル種類を選び、既存アイコンをソフト・拡張子ごとに保護します。`.lnk` と `.exe` は常に保護します。
- パックを切り替え、登録前の表示へ復元します。日本語・英語のGUIと補助CLIを使います。

取得だけではExplorerの表示を変えません。適用すると現在のWindowsユーザーの関連付けに作用するため、最初に[使い方](docs/usage.ja.md)を確認します。既存アイコンの保護は既定で有効であり、保護対象にはパックの画像を適用しません。

## 始め方と文書

[使い方](docs/usage.ja.md)で起動条件、取得、適用、本体の更新、復元、削除を確認します。自動化する場合は[CLI](docs/cli.ja.md)、仕組みを知りたい場合は[現行仕様](docs/current-specification.ja.md)を参照します。[文書一覧](docs/README.md)から利用・仕様・開発の資料を選べます。[変更履歴](CHANGELOG.md)は最初の公開版から記録します。

## 開発

WindowsのMSVC環境、Rust、PowerShell 7.4以降、[just](https://just.systems/man/en/)を用意します。`cargo install just --locked` でjustを導入し、`just --list` でコマンド一覧を表示します。

依存監査には`cargo install cargo-deny --version 0.20.2 --locked`で固定版を事前導入します。[更新と監査の方針](docs/dependency-security.ja.md)に従い、候補依存をビルドする前に`just audit`を実行します。

| コマンド | 用途 |
| --- | --- |
| `just run` | GUIとShell DLLをリリースビルドして起動する |
| `just run-debug` | デバッグ構成で起動する |
| `just cli --help` | 補助CLIのヘルプを表示する |
| `just verify` | 整形、静的解析、ビルド、操作・契約試験、起動確認を行う |
| `just audit` | 製品をビルドする前に、固定版cargo-denyで依存と例外期限を監査する |
| `just licenses` | 第三者の権利表示、選択結果、閲覧用HTMLを生成する |
| `just check-binaries --folder <フォルダー>` | EXE・DLLに残るビルド元のパスを検査する |
| `just package` | 検証用ZIPを作成します。新しい公開先では`--public-candidate`で公開候補を明示します |

GUIの起動だけではShell登録を変更しません。通常の起動・CLIは利用者の設定を使い、`just smoke` と自動試験は専用の領域を使います。ビルド、試験、配布確認の詳細は[開発と検証](docs/development.ja.md)、変更時の規約は[貢献手順](CONTRIBUTING.md)を参照します。

## ライセンス

開発時の権利表示生成には`cargo-about 0.9.2`を使います。`cargo install cargo-about --version 0.9.2 --features cli --locked`で導入します。配布物へ同梱する原文・ソースと生成結果の範囲は[配布手順](docs/distribution.ja.md#権利表示の収集範囲)を参照します。

本体は[MIT License](LICENSE)です。外部アイコンパック、依存ライブラリ、言語対応表には、それぞれの利用条件が適用されます。[第三者の権利表示](THIRD_PARTY_NOTICES.md)を参照します。アイコンパックの画像はソースリポジトリへ同梱しません。本体のMITは外部素材の再配布を一律に許可するものではありません。

配布版のGUIでは「アプリ情報」から本体と第三者のライセンスを開けます。開発版での準備は[開発手順](docs/development.ja.md#権利表示の生成)を参照してください。
