# 変更履歴 / Changelog

公開版の変更、更新方法、制限を0.1.0から記録します。日付は日本時間（UTC+09:00）です。

Changes, update instructions, and limitations for public releases are recorded from 0.1.0. Dates use Japan Standard Time (UTC+09:00).

## [0.2.0](https://github.com/YukiWorks432/FileHue/releases/tag/v0.2.0) — 2026-11-16

### 日本語

#### 主な変更

- AI・EPS・SVG・PSD・PSB・PDFの内容サムネイルを追加しました。「ファイルのプレビュー」で種類ごとに有効化できます。埋め込み画像などの条件によって表示できないファイルもあります。[対応範囲](docs/thumbnails.ja.md)
- アイコンの保護設定に検索、現在と保存後の比較、おすすめ設定を追加しました。拡張子のないファイルの設定も探しやすくしました。
- パック取得、プレビュー、画面移動と状態表示を整理し、アイコン見本の表示品質を改善しました。
- 保存済みのパック・対象選択・保護設定を維持して、アイコン用DLLを更新できるようにしました。
- 一部の正常なアプリ関連付けで、アイコンの適用や復旧が止まる問題を修正しました。
- GPUI Kit、SVG描画などの依存ライブラリを更新しました。既存パックは本体更新だけでは再変換しません。

#### 0.1.0からの更新

1. 旧版のGUI・CLIを終了します。事前に「Windows標準に戻す」必要はありません。
2. 新版ZIPを別フォルダーへ展開します。実行ファイルと両DLLを一緒に保持してください。
3. 新版を起動し、「取得済み」に「アイコン機能の更新」が表示されたら「更新する」を選びます。

設定の初期化やパックの再取得は不要です。サムネイルは別途有効にします。0.1.0へ戻す場合は、新版で先にサムネイルを解除してください。退避や競合時の対処は[更新手順](docs/usage.ja.md#アプリ本体の更新)を参照してください。

Windows 11 x64向けの未署名ZIPです。配布元とSHA-256を確認してください。ファイル内容と既定アプリを維持し、`.lnk`と`.exe`は常に対象外です。

### English

#### Highlights

- Added content thumbnails for AI, EPS, SVG, PSD, PSB, and PDF. Enable each format separately in the file-preview screen. Some files require embedded previews or other supported content. [Supported scope](docs/thumbnails.en.md)
- Added protection search, current-versus-saved previews, and recommended settings. Extensionless-file settings are easier to find.
- Improved pack downloads, previews, navigation, status displays, and icon preview quality.
- Added icon DLL updates that preserve downloaded packs, target selections, and protection settings.
- Fixed valid application associations that could block icon application or recovery.
- Updated GPUI Kit, SVG rendering, and other dependencies. Updating the application alone does not reconvert existing packs.

#### Updating from 0.1.0

1. Close the old GUI and CLI. Restoring the original icon registrations first is not required.
2. Extract the new ZIP into a separate directory. Keep the executables and both DLLs together.
3. Start the new version and use the update action when the downloaded-packs screen shows the icon-support update notice.

No settings reset or pack download is required. Enable thumbnail formats separately. Before reverting to 0.1.0, restore thumbnail registrations using the new version. See the [update guide](docs/usage.en.md#updating-filehue) for backups and conflicts.

Distributed as an unsigned Windows 11 x64 ZIP. Verify the source and SHA-256 checksums. File contents and default applications are preserved; `.lnk` and `.exe` remain excluded.

## [0.1.0](https://github.com/YukiWorks432/FileHue/releases/tag/v0.1.0) — 2026-11-05

### 日本語

初回公開版です。以下は0.1.0時点の機能と制限です。

GUI・CLI・Shell DLLにはコード署名を付けません。SmartScreenの警告が表示されたり、Smart App Controlや組織の方針によって実行が制限されたりする場合があります。配布元と`SHA256SUMS.txt`を確認してください。SHA-256は内容の照合用であり、発行者を認証するWindowsの署名ではありません。証明書の信頼登録や保護機能の広範な無効化を導入条件にはしません。

#### 主な機能

- Windows 11 x64の標準エクスプローラーで、ファイル名・拡張子に応じたVS Code形式のアイコンテーマを使います。
- 日英のGPUI製GUIで、確認済みパックの取得、手元のテーマの取り込み、プレビュー、適用・切替、復元・削除を行います。補助CLIも提供します。
- 適用対象をカテゴリ・名前・拡張子ごとに選び、ソフト・種類単位で既存アイコンを保護します。`.lnk` と `.exe` は常に保護します。
- ファイル内容と既定アプリを維持し、未対応の種類は通常表示へ戻します。取得・展開・変換に上限を設け、変更前の登録と復元記録を保持します。

#### 起動と制限

[使い方](docs/usage.ja.md)に従い、実行ファイルとShell DLLを隣接させ、必要なVisual C++ランタイムを導入します。初回の適用後はExplorerの再起動が必要になる場合があります。元の登録が指す画像を外部アプリが削除していた場合、復元は画像自体を修復しません。

0.1.0の公開時は、旧版で復元してから別フォルダーの新版を起動する更新手順を案内していました。0.2.0への更新には、上の0.2.0の節の手順を使用してください。パックの更新とは別の操作です。削除時の設定と復元記録の扱いは[使い方](docs/usage.ja.md#削除と言語)で確認してください。

Windows 10、ARM64、システム全体への導入、フォント型テーマ、すべての第三者ハンドラーや表示倍率は対応保証に含めません。サムネイル生成機能を追加するものではありません。ANSIのみの既存ハンドラーとの互換性は未確認です。COM経路の性能測定と、実Explorer全体の操作時間は区別します。

既存アイコンの保護、適用対象、テーマ定義によっては専用画像が表示されません。GUIから除外理由と見本を確認します。問題が起きた場合は[競合復旧](docs/conflict-recovery.ja.md)を参照し、復元記録を手動で消しません。

### English

Initial public release. The features and limitations below describe version 0.1.0.

The GUI, CLI, and Shell DLL are not code-signed. SmartScreen may show a warning, and Smart App Control or organizational policies may prevent execution. Check the distribution source and `SHA256SUMS.txt`. SHA-256 verifies file contents; it is not a Windows publisher signature. Installing a self-signed certificate or broadly disabling Windows protection is not a setup requirement.

#### Features

- VS Code-style filename and extension icons in Windows 11 x64 File Explorer.
- A Japanese/English GPUI GUI for reviewed-pack downloads, local imports, previews, applying and switching packs, restoration, and removal, plus an auxiliary CLI.
- Target selection by category, filename, or extension, with existing-icon protection by provider and file type. `.lnk` and `.exe` always remain protected.
- Preservation of file contents and default applications, Windows fallback for unsupported types, bounded download/extraction/conversion, and saved registration recovery information.

#### Setup and limitations

Follow the [usage guide](docs/usage.en.md). Keep the executables and Shell DLL together and install the required Visual C++ runtime. Explorer may need restarting after the first application. Restoration cannot recreate original artwork that another application has already removed.

At the 0.1.0 release, the documented application-update procedure was to restore using the old version before starting the new version from a separate folder. For updating to 0.2.0, use the instructions in the 0.2.0 section above. Pack updates are separate. See the usage guide for [removal and retained data](docs/usage.en.md#removal-and-settings).

Windows 10, ARM64, system-wide installation, font-based themes, and all third-party handlers or display scales are outside the supported scope. The application does not add a thumbnail provider. ANSI-only icon handlers have not been verified. COM-path performance measurements do not measure all Explorer interactions.

Protected types, disabled targets, or theme definitions may prevent a dedicated icon from being shown. Review exclusions and previews in the GUI. If recovery is needed, follow the [recovery guide](docs/conflict-recovery.ja.md) (Japanese) and the English recovery section of the [usage guide](docs/usage.en.md#recover-from-registration-conflicts). Do not delete recovery records manually.
