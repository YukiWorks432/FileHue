# 補助CLI

Windows 11 x64用の `file-icons.exe` は、GUIと同じパック一覧、設定、登録・復元処理を使うコンソール用の入口です。配布ZIPにはGUI、CLI、Shell DLLを同じフォルダーへ収録します。CLIからも日常の管理操作を実行でき、GUIの操作にCLI入力は必要ありません。

## 基本操作

配布フォルダーでPowerShellを開きます。識別子には一覧が返す完全な値を指定します。空白を含むパスは引用符で囲みます。

```powershell
.\file-icons.exe --help
.\file-icons.exe catalog list
.\file-icons.exe catalog show vscode-icons
.\file-icons.exe pack download vscode-icons
.\file-icons.exe pack list --json
```

取得は公式一覧の固定版だけを対象とします。任意URL、上限解除、検証の省略は受け付けません。[取得の安全上限](pack-downloads.ja.md)はGUIと共通です。取得後もアイコンは適用されません。

ローカルの展開済みVS Code拡張は、候補の識別子を確認して取り込みます。複数テーマの対話選択や、パッケージ内のプログラムの実行は行いません。

```powershell
.\file-icons.exe pack inspect "C:\themes\my theme"
.\file-icons.exe pack import "C:\themes\my theme" --theme "theme-id"
```

以下の `<pack-id>` は `pack list` が返す完全な識別子へ置き換えます。コマンドの山括弧をそのまま入力しません。

```powershell
.\file-icons.exe pack show "<pack-id>"
.\file-icons.exe resolve "<pack-id>" "Cargo.toml" --json
.\file-icons.exe apply "<pack-id>" --dry-run
.\file-icons.exe apply "<pack-id>"
.\file-icons.exe status --json
.\file-icons.exe restore --dry-run
.\file-icons.exe restore
```

`pack show` は保存済みの出典と権利表示を省略せずに出します。`resolve` はファイル内容を開かず、Explorerと同じ規則・画像の存在確認を使います。選択理由は `file_name`、`file_extension`、`language_file_name`、`language_pattern`、`language_extension`、`windows_default` のいずれかです。最後の値はテーマから有効な画像を返せない場合を表します。

取得・取り込み時に未定義のアイコンを参照する規則を除外した場合は、日英の警告と `data.warning_count` を返します。`pack list` と `pack show` のパック情報にも `warning_count` を含めます。`pack show --json` の `data.provenance.warnings` は、除外した規則の `rule_type`、`rule`、`icon_id` を保持します。これらのキーと識別子は翻訳しません。警告があっても有効な規則を取り込めれば終了コードは0であり、取得だけで適用はしません。

## 変更予定、復元、削除

`--dry-run` は `apply`、`restore`、`recover`、`pack remove`、`cleanup` で使えます。設定保存、変換、登録、削除は行いません。整合する読み取りのために管理ロックとそのフォルダーを作成することはあります。適用予定には対象の関連付けクラスと既存の復元記録、削除予定には共有する保存先の有無を含めます。配置DLLや素材の所有範囲・内容・削除権限は実行時に改めて検証するため、予定表示は後の実行成功を保証しません。

`apply` はパックの切り替えも扱います。既定アプリは変更せず、既存と競合した値を上書きしません。`restore` は本アプリによる変更を元へ戻します。設定を読めない場合は通常操作を止め、独立した復元記録が使えるときだけ次の手順を使います。

```powershell
.\file-icons.exe recover --dry-run
.\file-icons.exe recover
```

破損時の復元は元の設定を退避します。健全な管理用一覧と言語は保持するが、壊れた一覧は修復しません。未知の設定形式を強制上書きする機能も設けません。

`pack remove` は一覧と、そのパックだけが参照する所有済みの生成物を削除します。元の拡張フォルダーを削除せず、適用中のパックは削除できません。ロック等で削除が残る場合は記録を維持し、復元後の `cleanup` で再試行します。`cleanup` は配置済みの未使用DLLも整理します。登録解除そのものとは別の操作です。

```powershell
.\file-icons.exe pack remove "<pack-id>" --dry-run
.\file-icons.exe pack remove "<pack-id>"
.\file-icons.exe cleanup --dry-run
.\file-icons.exe cleanup
```

Explorerは自動終了しません。適用・復元後に表示が更新されなければ、コピーや移動を終えてからタスク マネージャーでExplorerを再起動します。

## 出力と言語

`--json` は標準出力へ一つのJSONを出します。成功は `schema_version: 1`、`ok: true`、`data`、`warnings`、失敗は `schema_version: 1`、`ok: false`、`error` を持ちます。取得の進捗は標準エラーへ出します。機械処理では案内文を解析せず、`error.code` と終了コードを使います。

```json
{"schema_version":1,"ok":false,"error":{"code":"manager_busy","message":"別の管理操作が実行中です。GUIを閉じて再試行してください。","details":"Busy"}}
```

`--lang ja` / `--lang en` は今回の表示だけを変え、保存済みのGUI言語は変えません。指定しない場合は排他して読めた保存済み言語を使い、未設定・破損・使用中では日本語へ補完します。キー、識別子、状態値は翻訳しません。OSや下位処理の診断は詳細として保持します。`--` 以降はオプションではなく位置引数として扱います。未知のコマンド、余分な位置引数、重複オプション、対応しないオプションは失敗します。

| 終了コード | 意味 | 主なerror.code |
| --- | --- | --- |
| 0 | 成功、ヘルプ、変更予定の表示 | なし |
| 2 | 引数不正 | invalid_arguments |
| 3 | 他の管理操作が使用中 | manager_busy |
| 4 | 対象なし、前提条件不成立 | not_found / precondition_failed |
| 5 | 通信・入出力・変換等の失敗、削除保留 | operation_failed / cleanup_pending |
| 6 | 登録の競合 | registration_conflict |
| 7 | 設定または登録の復旧が必要 | recovery_required |
| 130 | 中断 | cancelled |

`status` の `registration_state` は `inactive`、`active`、`recovery_required` を返します。状態の照会に成功して問題を報告できた場合も終了コードは0です。設定そのものが読めなければ7となります。`pending_pack_removals` は再試行対象を表します。

登録の事前確認で競合を検出した場合、GUIとCLIの詳細には対象のHKCUキー、値の名前、読み込んだ設定ファイルの場所を表示します。この段階では登録を書き換えません。既定値の名前は `(Default)` と表します。外部アプリの変更は保持し、復元記録を削除して競合を回避しません。再起動やビルド成果物の削除では、保存済みの登録状態は変わりません。

## 確認付きの競合復旧

`conflicts list --json` で全登録と外部変更を照会できます。`recover --strategy preserve-external --dry-run --json` または `recover --strategy reapply-theme --dry-run --json` で変更予定と確認用識別子を取得し、同じ方法に `--confirm <confirmation>` を指定して実行します。前者は外部変更を保持して解除し、後者は外部値を新しい解除先へ退避してパックと保存済み選択を再適用します。確認後の状態変化は終了コード6、未完了の復旧・不正な記録等は7で報告します。無条件の強制上書きは受け付けません。完全なJSON項目と再試行手順は[登録競合の復旧](conflict-recovery.ja.md)を参照します。

## 排他と中断

GUIは起動中の管理ロックを保持します。CLIの状態照会と変更も同じ排他を取得するため、GUIを閉じてから使います。カタログ照会、ローカルの候補解析、ヘルプ、版の表示は保存済み状態に依存せず、使用中でも実行できます。CLIは入力待ちや昇格のための対話を行いません。

Ctrl+C / Ctrl+Breakは中断要求として受け付けます。取得・変換は安全な確認箇所で停止し、未完成のパックを一覧へ加えません。登録・復元・削除の確定中は処理を途中で打ち切らず、安全な境界まで進めます。この場合、処理が完了すれば成功を返します。強制終了からの復旧は保存済みの独立記録を使います。

## 検証

実CLIの子プロセスで引数、JSON、終了コード、日英、排他、取り込み・照合・削除、変更予定が状態を変えないことを確認します。登録・切替・競合・破損復旧はプロセス専用レジストリへ振り向けます。GUIとの共通化はGPUIのtest-supportも通します。公式配布元を使う試験は明示指定時だけ実行します。

```powershell
cargo test -p file-icons-cli --locked
cargo test -p file-icons-cli --test cli --locked -- --ignored --exact official_downloads_through_real_cli_keep_registration_inactive
```

開発時の重い検証は、共通のCPU排他を通します。実Explorerでの表示受け入れと隔離したCLI試験は区別します。

## アイコンの適用対象

パックごとのカテゴリ・拡張子・固有名の選択、一括編集とCLIの操作は[適用対象の選択](target-selection.ja.md)を参照します。保存形式はShell用と管理用のいずれも形式4です。非対応形式は自動で上書きしません。

`protection show` で既存アイコンの保護設定、`protection exclusions <pack-id>` で次回適用時の除外種類・理由・登録位置を確認できます。`protection set on|off [--dry-run]` で全パック共通の設定を保存します。既定は有効で、適用中なら復元・再適用まで行います。`--dry-run` は保存も登録変更も行いません。[判定と保存形式](target-selection.ja.md#既存アイコンの保護設定)を参照します。

## Windows固有アイコンの保護

`.lnk` と `.exe` はWindowsの固有アイコンを維持します。適用対象のチェックは変更不可で、一括操作やコピーでも有効になりません。CLIで明示的に有効化すると、変更を保存せずに前提条件エラーを返します。JSONの対象一覧にはprotected、解決結果にはreason: protected_file_typeを表示します。旧版の登録を解除するにはテーマを明示的に再適用します。他者による変更があれば競合として報告し、上書きしません。旧DLLやキャッシュが残る場合はExplorerを再起動します。アプリの起動だけでは登録を変更しません。

GUIで保存した提供元・拡張子別の例外は、適用・復旧・`protection exclusions` でも反映します。`protection set on|off` は個別の例外を保持して既定の方針を変更します。元の指定が消えた場合も再確認まで保護し、除外理由には `changed_registration` を返します。
