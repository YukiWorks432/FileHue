# 開発者向けの「おすすめ設定」を一つのボタンで反映する

作成・改訂日: 2026-11-05。調査対象: `cb7ede3c173d6685fa2e104546cc66b3e248aa4b`。優先度P2、作業量は中。別計画への依存はない。本計画を実装の基準とする。実施した検証と残存事項はPR本文へ記録する。

## 目的と変更後の操作

保護画面の「おすすめ設定」を一度押すと、開発者向けの一組の設定をまとめて反映する。VS Codeが提供する既存アイコンや、代表的なプログラミング用拡張子について、既存アイコンよりパックを優先する。利用者に項目の組み合わせや拡張子の選定を求めない。

ボタンを押した時点で通常の設定欄と表示予定を更新し、その後はいつもの操作で個別に調整できる。「保存」または「保存して再適用」で確定する。追加の選択画面や確認ダイアログは置かない。

以下の方針を一括設定の仕様案とする。

| 対象 | おすすめ設定での扱い |
| --- | --- |
| 全体の方針 | 「既定では既存アイコンを保護する」を有効にし、開発向けの対象だけにパックの使用を許可する |
| VS Code由来のアイコン | 調査済みの提供元の基本設定をパック優先へ変更する。対象の個別指定も継承へ戻し、以前の個別保護が残って効果を妨げないようにする |
| ソースとスクリプト | 提供元に関係なく、プログラミング用拡張子をパック優先にする |
| 開発用の設定・ビルド・文書 | よく使う設定形式、開発プロジェクト、Markdown等もパック優先にする |
| 画像・動画・音声・フォント・Office文書等 | 拡張子の用途を理由とする一括解除の対象にしない。VS Code由来と確認できる場合を除き、既存の個別方針と通常の保護に従う |

これは現在の設定に開発者向けの方針を上書きする操作である。おすすめ対象の明示的な個別保護も変更し、対象外の個別指定や別ソフトの基本設定は保持する。全体設定がオフの場合はオンへ戻ることを、ボタン付近の短い説明で示す。

## 開発用拡張子の選び方

TypeScriptの数種類だけに限定せず、既存の用途分類と言語定義を基礎に広く対象を決める。次の表は初版の選定規則であり、例だけを固定した小さな一覧に置き換えない。

| 選定規則 | 対象の例 |
| --- | --- |
| 既存の `language` 分類 | C/C++、C#、Rust、Go、Python、Java、Kotlin、Swift、Ruby、Lua、Dart、SQL、シェル等のソースとスクリプト |
| 既存の `web` 分類からフォントを除いたもの | JavaScript・TypeScript・JSX・TSX、HTML・CSS、PHP、Vue・Svelte等。`.ts`・`.tsx`・`.mts`・`.cts` を含む |
| 開発でよく使うデータ・設定形式 | `.json`・`.jsonc`・`.json5`・`.jsonl`・`.yaml`・`.yml`・`.toml`・`.xml`・`.ini`・`.properties`・`.env` |
| 既存の `build`・`tools` 分類の拡張子 | `.cmake`・`.gradle`・`.csproj`・`.sln`・`.diff`・`.patch` 等 |
| 開発文書としてよく使う形式 | `.md`・`.mdx` |

`web` にはフォントも含まれるため、カテゴリをそのまま一括許可しない。`.woff`・`.woff2`・`.ttf`・`.otf` は拡張子による推薦から除外する。`data`・`documents` 全体も許可しない。CSV、データベース、PDF、Office文書等まで対象が広がるのを防ぐ。

判定はアイコンの提供元や現在の既定アプリではなく、ファイル拡張子の開発用途に基づく。このため、メディア用ソフトに関連付いた `.ts`・`.mts` もパック優先になる。拡張子を共有する動画とソースの内容は識別しない。`.m2ts`・`.mp4` のような別拡張子まで許可を広げない。

固有名の分類から、その拡張子全体を許可してはならない。たとえば `CMakeLists.txt` が開発用でも、あらゆる `.txt` を許可する理由にはしない。拡張子なしの開発ファイルを扱うために `Unknown` 全体の保護を解除することも避ける。名前別の保護例外はこの計画では新設しない。

## 現在の実装と接続先

- [categories.rs](../crates/icon-core/src/categories.rs): 同梱の分類表と言語定義から用途を判定する。`category(kind, name)` はクレート内部の関数である。
- [categories.json](../crates/icon-core/data/categories.json)・[languages.json](../crates/icon-core/data/languages.json): 現在の用途と言語拡張子の正本。表示用の `web` にフォントが含まれる。
- [selection.rs](../crates/icon-core/src/selection.rs): `CompiledTheme::targets()` が `kind`・`name`・`category` を持つ対象一覧を返す。今回は対象選択の有効・無効を変更しない。
- [icon_policy.rs](../crates/icon-windows/src/icon_policy.rs): `IconProtectionRules` が提供元別と拡張子別の指定を保持する。`set_provider` と `set_extension` は確認済みの登録を表す `fingerprint` を設定する。
- [protection_ui.rs](../apps/icon-manager/src/protection_ui.rs): `Editor` が全体設定の下書き `draft`、個別の下書き `rules`、調査済みの `sources` を所有する。`protection_panel` が保護画面、`refresh_protection_policy` が取得済み情報からの表示更新、`save_protection` が保存の入口である。
- [protection_sources.rs](../apps/icon-manager/src/protection_sources.rs): 通常の三択操作は `pin_visible_protection_rows`、ルール変更、`refresh_protection_policy` の順に行う。この経路へ合わせる。
- [管理サービス](../crates/icon-manager-service/src/lib.rs): `set_icon_protection_rules` が登録競合を検査し、適用中なら既存の復元・再適用処理へ進む。新しい保存・登録経路は不要である。

現在の設定型は次のように提供元と拡張子を分けている（`icon_policy.rs` の13行目付近）。

```rust
pub struct IconProtectionRules {
    pub providers: BTreeMap<String, BTreeMap<String, ProtectionRule>>,
    pub extensions: BTreeMap<String, ProtectionRule>,
}
```

設定の優先順位は、拡張子の個別指定、提供元の基本設定、全体設定の順である。VS Codeの基本設定を変えるだけでは、個別保護が残ってパック優先にならない場合がある。おすすめ操作では、対象の実効方針がパック優先になるところまで一括で設定する。通常の親の三択が個別指定を保持する既存契約は変えず、この一括操作の効果として明示する。

## 一括反映の契約

1. 共通設定カードに「おすすめ設定」の実行ボタンを一つ置く。付近に「開発用ファイルをパック優先にし、ほかの種類は既定で保護する。保存前に個別調整できる」という趣旨を短く表示する。
2. ボタンを押すと `Editor.draft` を `true` にし、VS Codeと開発用拡張子の指定をまとめて更新する。変更結果を通常の三択・件数・表示予定へ直ちに反映する。
3. 調査済み情報全体を対象とする。現在の検索・状態絞り込み・提供元の開閉によっておすすめの結果を変えない。変更中の表示行は通常操作と同じく保持する。
4. 判定できたVS Code提供元には `set_provider(..., Some(false))` を使い、その対象行の拡張子別指定を解除して基本設定を継承させる。それ以外の推薦拡張子には `set_extension(..., Some(false))` を使う。同じ種類へ不要な二重指定を追加しない。
5. 対象外の下書きを保持する。対象に含まれる既存の個別保護は、このボタンの一括設定で置き換える。おすすめ反映後の手動調整は通常の三択で行える。
6. 同じ状態で何度押しても設定や指定件数が増えない。設定変更がなければ「おすすめ設定になっています」等の短い状態を示す。現在の設定から算出し、適用済みという永続フラグを作らない。
7. 結果は「開発者向けのおすすめ設定を反映しました。変更○種類・要確認○種類」等で通知し、既存の未保存表示を使う。追加の確認ダイアログや別の確定ボタンは置かない。
8. 再調査・画像抽出・保存・再適用はボタンから起こさない。通常の「保存」「保存して再適用」が確定操作となる。保存せず戻れば保護の下書きを破棄し、保存失敗では下書きを保持する。
9. 調査中・調査失敗・設定無効・処理中は実行できない。画像の見本だけが読み込み中・取得不能でも、登録調査が成功していれば実行できる。

## 保護境界と対象範囲

`.lnk`・`.exe`、`always_protected` の種類は常時保護を維持する。登録変更で `needs_review` となった種類や元指定が消えた種類は、一括操作で再許可せず従来の個別確認へ残す。通常の競合検査と、共有クラス内の拡張子ごとの保護・元画像への委譲も維持する。

VS Codeは調査済みの `provider_id` と `provider_location` から提供元の実行ファイルを確認する。初版の具体的な照合対象は `Code.exe` とし、大文字・小文字を区別しない。表示名の部分一致や、既定アプリ名だけで別の画像提供元をVS Codeと断定しない。画像資源だけで関係を確認できない場合は、開発用拡張子の規則と通常の手動操作で扱う。別製品の実行ファイル名まで黙って対象を広げない。

対象は確認中パック、共有する関連付け、保存済みの指定など、今回調査済みの種類に限る。パックが対応しない種類や、パック内で利用者が対象外にした種類を有効化しない。保存する方針は従来どおり全パック共通である。新規パックや外部アプリの変更時におすすめを自動実行する機能は追加しない。

## 実装対象と手順

一つの機能、一つの変更単位として実装する。実装前に `AGENTS.md` と関連文書を読み、`git status --short` と対象差分を確認する。計画の基準コミットからの差分があれば、型・分類・保存経路を照合し直す。

### 1. 推薦方針を定義する

`crates/icon-core/src/recommendations.rs` を追加し、既存の `categories::category` を使う開発用拡張子の判定を置く。[lib.rs](../crates/icon-core/src/lib.rs) から公開し、GUIで内部の分類表を複製しない。フォント除外と、上表の設定形式・Markdownの追加だけをこの推薦方針で扱う。表示用分類そのものや同梱言語定義は変更しない。

[要求仕様](../docs/requirements.ja.md) と [保護の詳細仕様](../docs/target-selection.ja.md) に、一つのボタンで反映する範囲と個別保護の置き換えを記録する。

確認: `cargo test -p file-icons-core --locked recommendations` で、具体例に基づく対象・対象外の試験が1件以上成功すること。`just check-docs` と `git diff --check` が成功すること。

### 2. おすすめ設定ボタンを下書きへ接続する

`apps/icon-manager/src/protection_recommendations.rs` を `protection_ui.rs` 配下のモジュールとして追加し、調査済みの型から一括反映する処理をまとめる。ボタンは既存の `theme::button`、文言は `self.text(日本語, 英語)` に合わせる。追加の選択状態や設定画面は作らない。

反映前に実行可否を確認し、表示行の固定、全体設定・ルールの一括更新、`refresh_protection_policy` を一度呼ぶ順とする。変更件数は種類を重複なく数え、全体設定だけが変わる場合も未保存状態を正しく示す。単一機能のための汎用規則エンジンや新しいキャッシュは作らない。

確認: `cargo test -p file-icons-manager --features test-support --locked protection_recommendations` で、推薦反映の試験が1件以上成功すること。`just fmt-check` と `just clippy` が成功すること。

### 3. 操作・保存・表示の契約を検証する

`apps/icon-manager/src/ui_tests/protection_recommendations.rs` を追加し、[ui_tests.rs](../apps/icon-manager/src/ui_tests.rs) へ登録する。[protection_sources の試験](../apps/icon-manager/src/ui_tests/protection_sources.rs) と同じく、親試験が専用の `LOCALAPPDATA` と隔離レジストリを用意して子試験を起動する。

確認: `cargo test -p file-icons-manager --features test-support --locked ui_tests::protection_recommendations::recommended_protection_gui_in_isolated_profile -- --exact --nocapture` が成功すること。無視指定の子試験を通常プロファイルで直接起動しない。

日英・明暗・通常幅・640×640・基準文字18pxで、ボタン、説明、結果、個別調整、保存・戻るへの到達性を描画画像で確認する。既存の [target_visual.rs](../apps/icon-manager/src/ui_tests/target_visual.rs) の方法を使い、画像は `work/` へ保存して実際に読む。

### 4. 正本と利用案内を合わせて最終検証する

[現行仕様](../docs/current-specification.ja.md)、[UI規約](../docs/ui-design.ja.md)、[日本語の使い方](../docs/usage.ja.md)、[英語の使い方](../docs/usage.en.md) を実装に合わせる。[計画一覧](README.md) と本計画の状態も更新する。

確認: `just verify`、`just check-docs`、通常権限・専用領域での `just test-shell` が成功すること。重い試験は利用可能な `cpu-benchmark-lock` の手順でほかの試験・計測と重ねない。ビルド単独は共通排他の対象外である。

上記以外のWindows側設定型、保存形式、CLI、管理サービス、Shell DLL、共通テーマ、依存版の変更は予定しない。通常プロファイルへのShell登録と実Explorerの確認は、明示的な導入・検証の依頼を受けて別途行う。

## 独立した具体例で確かめること

`avoid-tautological-tests` に従い、実装の候補一覧を期待値へコピーせず、仕様から導く次の操作結果を検証する。

- VS Code由来の `.rs`・`.json`・`.md` がパック優先になる。以前の `.md` の個別保護もおすすめ操作で解除され、通常の親の三択では個別指定を保持する従来動作と区別できる。
- 別のアプリが提供元でも、`.py`・`.c`・`.cpp`・`.cs`・`.java`・`.kt`・`.go`・`.js`・`.tsx`・`.sql`・`.ps1` 等がパック優先になる。TypeScriptの4種類だけに実装を狭めた場合に失敗する例を含める。
- 動画用の関連付けでも `.ts`・`.tsx`・`.mts`・`.cts` はパック優先になり、同じ提供元や関連付けクラスを共有する `.m2ts`・`.mp4` は保護を維持する。
- `.json`・`.yaml`・`.toml`・`.csproj`・`.patch`・`.md` 等も推薦される。一方、別提供元の `.woff2`・`.png`・`.psd`・`.pdf`・`.docx`・`.csv`・`.sqlite` は新しく許可しない。
- `CMakeLists.txt` の存在から `.txt` 全体へ許可を広げず、拡張子なしの名前を理由に `Unknown` 全体を許可しない。
- 全体設定がオフでも、ボタン一回でオンになり、開発向けの例外と整合する。対象外の明示的な個別指定と無関係な下書きを保持する。
- `.lnk`・`.exe`、要確認の旧許可、消えた元指定は一括で許可しない。VS Codeの表示名だけを持つ別資源を提供元の一致として扱わない。
- クリックまたはTab・Enter・Spaceの通常操作で、追加の選択画面を経ずに反映できる。絞り込み・提供元の開閉によって結果を変えない。再度押しても結果が変わらない。
- ボタンを押しただけでは設定ファイル・登録・調査番号・画像を変えない。保存後は設定が残り、保存せず戻れば従前の設定へ戻る。保存失敗時は競合を上書きせず下書きを保持する。

## 完了条件と報告

- [ ] 一つの「おすすめ設定」ボタンで開発者向けの方針を一括反映でき、上の具体例が成功する。
- [ ] 反映後の個別調整、保存、破棄、保存失敗をGPUIの実操作で確認している。
- [ ] `just verify`、`just check-docs`、`just test-shell` が成功し、対象コミット・実行条件・終了コードを報告している。
- [ ] 日英・明暗・640×640・18pxの画像を実際に確認し、保存場所と結果を報告している。
- [ ] 要求仕様、詳細仕様、現行仕様、日英の利用案内が最終実装と一致している。
- [ ] `git diff --check` と対象差分を確認し、計画外の変更がない。

設計では [project-ui-design](../.agents/skills/project-ui-design/SKILL.md)、[貢献手順](../CONTRIBUTING.md)、[固定版資料の参照手順](../docs/design-references.ja.md) に従う。計画時には既に固定版 `5591a658e460a404568ab1372aa589c1b5fdad41` のGPUI Kit資料を確認している。使用するAPIは採用中の `gpui-kit = "=0.7.3"` のソースで確認する。

実施した検証の成否と表示確認の結果はPR本文を参照する。保存形式の変更、要確認の自動解除、未知の提供元の推測などが必要になれば、その理由を示して計画を見直す。作業ブランチが必要なら `feat/recommended-icon-protection` を使い、コミット・送信・PRは実装時の許可範囲に従う。
