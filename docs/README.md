# 文書一覧

現在の仕様と再現可能な手順を収録します。利用者向けの使い方、CLI、初回リリースの案内は日本語と英語を用意し、開発・設計文書は日本語で管理します。

| 読みたい内容 | 文書 |
| --- | --- |
| 起動、取得、適用、復元、削除 | [使い方](usage.ja.md) / [English](usage.en.md) |
| コマンドとJSON出力 | [CLI](cli.ja.md) / [English](cli.en.md) |
| 初回版の機能と制限 | [0.1.0](releases/0.1.0.ja.md) / [English](releases/0.1.0.en.md) |
| 公開後の変更 | [変更履歴](../CHANGELOG.md) |
| 要求と受け入れ基準 | [要求仕様](requirements.ja.md) |
| 動作の全体像 | [現行仕様](current-specification.ja.md) |
| 拡張子・名前の選択、既存アイコンの保護 | [適用対象と保護設定](target-selection.ja.md) |
| 登録競合、設定破損からの復旧 | [競合復旧](conflict-recovery.ja.md) |
| パックの取得元、互換性、安全上限 | [パック取得](pack-downloads.ja.md) |
| 構成と保存・登録の境界 | [アーキテクチャ](architecture.ja.md) |
| ビルド、試験、監査 | [開発と検証](development.ja.md) |
| 依存の7日待機、個別例外、ビルド時の実行許可 | [依存の更新と監査](dependency-security.ja.md) |
| ZIPと権利表示の収集 | [配布物の作成と検証](distribution.ja.md) |
| 実Explorerでの受け入れ | [検証手順](explorer-acceptance.ja.md) |
| Shell DLLの性能計測 | [性能検証](performance.ja.md) |
| UIの判断基準と参照方法 | [デザイン方針](design.ja.md)、[UI規約](ui-design.ja.md)、[外部資料](design-references.ja.md) |
| 変更の提出とレビュー | [貢献手順](../CONTRIBUTING.md)、[デザインスキル](../.agents/skills/project-ui-design/SKILL.md) |
| 本体以外の権利表示 | [第三者の権利表示](../THIRD_PARTY_NOTICES.md)、[補完原文の出典](../licenses/upstream/manifest.json) |

## 検証と権利表示

[開発規約](../AGENTS.md)と配布手順に従い、対象コミットと成果物を照合します。文書整理だけで公開承認や権利確認が完了したとは扱いません。

過去の検証結果を別のコミットへ引き継がず、採用する版で確認します。ライセンス原文の出典・対応する版・照合情報は継続して保持します。
