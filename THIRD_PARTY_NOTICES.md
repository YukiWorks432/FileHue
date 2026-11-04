# 第三者の権利表示

日本語 | [English](THIRD_PARTY_NOTICES.en.md)

## VS Codeの言語対応表

`crates/icon-core/data/languages.json` は、Microsoftの公開リポジトリの言語定義から、言語識別子・拡張子・ファイル名・ファイル名パターンを抽出したものである。

- 出典: [Microsoft / VS Code](https://github.com/microsoft/vscode)
- 取得コミット: 対応表の `commit` を正本とする。再生成時には同じコミットの許諾文を取得し、`license_sha256` で対応を記録する。
- 個別の参照URLは対応表内の `sources` に記録する。
- 権利表示: VS CodeのMITライセンス。ソースでは `crates/icon-core/data/VSCODE-LICENSE.txt`、配布物では `licenses/VSCODE-LICENSE.txt` に収録する。
- 再生成: `scripts/update-language-catalog.ps1`

TOML・YAML・Zigの補完対応は本製品の初期要件に基づく独自の規則であり、上記の生成表には含めていない。

## 外部アイコンパックとRust依存

アプリで使うアイコンパック一式は、このリポジトリへ同梱しない。ソースのREADME比較画像と再現用HTMLにはvscode-iconsの素材を含むため、[比較画像の権利表示](https://github.com/YukiWorks432/FileHue/blob/main/docs/assets/explorer/NOTICE.md)で出典・CC BY-SA 4.0・ブランド素材の条件を別に記録する。取り込み時に、パックの宣言、下位フォルダーと明示参照を含むライセンス・通知・READMEをキャッシュの `provenance.json` へ保存する。公式配布物から取得した場合は、配布物のSHA-256、取得URL、版、利用条件の要約、ICOへの変換内容も保持する。素材は本体のMITへ変更しない。具体的な候補と条件は[パック取得の設計](docs/pack-downloads.ja.md)を参照する。

Rust依存の完全な一覧と配布時の通知は、配布物を作成する段階でCargo.lockと実際に配布する部品から整備する。この文書だけで全依存の権利表示を完了したとは扱わない。

配布物では`licenses/THIRD_PARTY_LICENSES.html`にライセンス全文と元の権利表示・通知を掲載し、提供ソースへのリンクを示す。ソースからは`just licenses`で同じ形式を生成する。cargo-aboutの条件選択と元の権利文書を併用する。開発時の生成手順はソース内の`docs/distribution.ja.md`に記録する。

mac 0.1.1はApache-2.0を選択し、取り込み元のMIT条件と権利表示も保持する。2026-10-25、FileHueの管理者は現行の補完内容と上流未確認の事項を承知し、この版の配布を採用すると決定した。選択根拠、採用判断、固定した出典、上流未確認の事項は`SELECTION.md`と`provenance.json`に記録する。ソースでは`licenses/upstream/mac-0.1.1/`に置き、配布物では第三者の権利表示HTMLから参照できる。
