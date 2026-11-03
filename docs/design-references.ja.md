# 外部デザイン資料の参照手順

外部スキルは個人環境への導入を前提にせず、固定版のURLから読めるようにする。本文、参照ガイド、翻訳、図版、コード例は本リポジトリへ同梱しない。本製品が採用する規則は[デザイン規約](ui-design.ja.md)に記載する。

## 参照する版

次の版は既存の設計記録で参照した版を引き継ぐ。GPUI Kitの資料の版は、アプリが依存するクレートの版を示すものではない。実装するAPIはCargo.tomlとCargo.lockに固定された版のソースで確認する。

| 資料 | コミットSHAと入口 | 読む対象 |
| --- | --- | --- |
| GPUI Kit Design Guides | `5591a658e460a404568ab1372aa589c1b5fdad41`：[スキル](https://github.com/longbridge/gpui-kit/blob/5591a658e460a404568ab1372aa589c1b5fdad41/skills/gpui-kit-design-guides/SKILL.md) | [Design Guides本文](https://github.com/longbridge/gpui-kit/blob/5591a658e460a404568ab1372aa589c1b5fdad41/skills/gpui-kit-design-guides/references/design-guides.md)の作業、配置、色、部品、状態、確認項目 |
| GPUI Kit | `5591a658e460a404568ab1372aa589c1b5fdad41`：[スキル](https://github.com/longbridge/gpui-kit/blob/5591a658e460a404568ab1372aa589c1b5fdad41/skills/gpui-kit/SKILL.md) | [Coding Guides本文](https://github.com/longbridge/gpui-kit/blob/5591a658e460a404568ab1372aa589c1b5fdad41/skills/gpui-kit/references/coding-guides.md)の状態所有、識別子、非同期、フォーカス、テスト |
| Web Design Guidelines | `063bee94c3f4df8453406c830b0a7df0f2860278`：[スキル](https://github.com/vercel-labs/agent-skills/blob/063bee94c3f4df8453406c830b0a7df0f2860278/skills/web-design-guidelines/SKILL.md) | 下記の別リポジトリのガイドを取得する入口 |
| Web Interface Guidelines | `434b7f91364665f2f733b310ec54809bf8f37937`：[ガイド本文](https://github.com/vercel-labs/web-interface-guidelines/blob/434b7f91364665f2f733b310ec54809bf8f37937/command.md) | 入力の説明、フォーカス、長い内容、処理中・失敗時の回復など、デスクトップにも当てはまる観点 |

## GUIの変更とレビューでの読み方

1. 本製品のデザイン規約と変更する画面の仕様を読む。GPUI Kitの2つのスキルを入口に、ガイド本文の変更に関係する節を読む。新しい画面や全体の再設計ではガイド本文全体を確認する。
2. 外部資料中の相対リンクは、その固定版のディレクトリを基点に解決する。別リポジトリやWebサイトを読む必要がある場合は、参照したURL・版・日付を記録する。Vercelの入口が最新取得を指示していても、本製品の基準には上表の固定版を使う。最新版との比較は別の調査として扱う。
3. Webの規則は目的を確かめ、GPUIの部品と入力方式へ対応させる。HTML、CSS、ARIA、ブラウザのURL管理をそのまま必須要件にしない。アクセスできない資料を参照済みとせず、不足した範囲をPRへ残す。
4. 外部資料と本製品の仕様が異なる場合は本製品の仕様を優先し、違いを記載する。フラットボタンの扱いが代表例である。採用中のAPIと異なる例を導入するために、依存版を無断で更新しない。
5. PRに参照した資料・SHA・該当節、本製品への適用判断、操作試験と表示確認の結果を記載する。参照版を変える場合は、変更理由、既存規約への影響、ライセンスの再確認を同じPRへ含める。

個人環境に別の版が導入されていても、共有する判断の根拠は本書の固定版とする。資料を取得できない場合も、本製品の規約だけで実施できる調査は続けられる。未確認の外部資料に適合したとは報告しない。

## 権利と公開の境界

GPUI Kitの[Apache-2.0許諾](https://github.com/longbridge/gpui-kit/blob/5591a658e460a404568ab1372aa589c1b5fdad41/LICENSE-APACHE)と[追加の文書許諾](https://github.com/longbridge/gpui-kit/blob/5591a658e460a404568ab1372aa589c1b5fdad41/LICENSE-DOCS.md)を分けて扱う。追加のCC BY 4.0は列挙された文書ディレクトリの対象文章・独自図版に限られ、skills配下全体やコード例へ一律に適用しない。

Vercelのスキルは[READMEのMIT宣言](https://github.com/vercel-labs/agent-skills/blob/063bee94c3f4df8453406c830b0a7df0f2860278/README.md#license)を、別リポジトリのガイドは[著作権表示を含むMIT許諾全文](https://github.com/vercel-labs/web-interface-guidelines/blob/434b7f91364665f2f733b310ec54809bf8f37937/LICENSE)をそれぞれ確認する。同一組織の資料という理由で、片方の許諾文をもう片方へ流用しない。

本製品独自の規約とスキルは本体のMITの対象とし、外部資料の権利をその表示で覆わない。将来、転載・翻訳・改変・同梱へ方針を変える場合は、対象ファイルと版ごとに適用条件、必要な許諾全文、権利表示、変更の明示を確認する。出典リンクだけで転載条件を満たしたと扱わない。
