# 配布物の作成と検証

FileHue 0.1.0のWindows x64向け配布物を作る。検証用と公開候補を明示的に分け、作成成功を実Explorerの受け入れや公開許可と混同しない。公開先は `https://github.com/YukiWorks432/filehue` とする。初回はGUI・CLI・Shell DLLを未署名で配布し、自己署名・仮署名も付けない。

## 作成と確認

配布候補を作る前に、[依存の更新と監査](dependency-security.ja.md)に従って`just audit`を実行する。監査の成功と権利表示の最終確認は別々に判断する。

PowerShell 7、RustのMSVCツールチェーン、cargo-about 0.9.2を使う。cargo-aboutは`cargo install cargo-about --version 0.9.2 --features cli --locked`で導入する。必要な依存パッケージを取得済みの環境で、ソースをコミットしてから次を実行する。ビルドと、その後の試験の実行条件は[開発手順](development.ja.md)に従う。

```powershell
just package
```

既定では固定したWindows x64ターゲットの最適化ビルドを作り、実行ファイルとShell DLLを隣接して配置する。日英の利用手順と第三者の権利表示、本体と外部依存のライセンス、元のVS Code言語対応表の権利表示を含める。第三者の権利表示は日英で相互に移動でき、配布時に同梱するHTMLへのリンクを生成する。デバッグ版の検証には `--profile debug` を指定できる。未コミットのソースで試す場合は `--allow-dirty` を明示し、その状態を配布物に記録する。

展開した配布物ではGUIの「アプリ情報」から版と同梱ライセンスを確認できる。公開先が実在することを確認した公開版では、`apps/icon-manager/src/about_ui.rs` の配布ページを設定する。

保存先は `dist/filehue-<版>-windows-x64-<用途>-<構成>-<コミット先頭12桁>/` で、用途は検証用の`validation`または公開候補の`candidate`である。この中へ展開内容の`payload/`、同名のZIP、`SHA256SUMS.txt`、検証用の`manifest.json`・`about.json`・`dependencies.json`とソース原本の`sources/`をまとめる。後者の記録と原本はZIPへ入れず、配布管理者がZIPと一緒に保持する。既存の出力は上書きしない。必要なファイルを明示してコピーし、設定ファイルやGit履歴を持ち込まない。PDBや取得済みアイコン素材は含めない。

形式5の外部`manifest.json`には製品名、用途、版、`signature=unsigned`、公開許可とは独立した`publication_approved=false`、ソースのコミットと変更状態、追跡ファイル一覧のハッシュ、Rust版、ターゲット、Cargo.lockと権利確認記録のSHA-256、ZIP内の全ファイルの相対パス・サイズ・SHA-256を記録する。Cargo.lockはソースに保持し、ZIPへ含めない。公開候補では公開リポジトリのURLも記録する。`SHA256SUMS.txt`にはZIPと外部マニフェストのSHA-256を記録する。展開した内容を再確認するには、外部記録を指定する。省略時は展開先の親フォルダーを使う。

```powershell
cargo xtask package --verify dist/<展開先> --metadata dist/<外部記録のフォルダー>
```

一覧外のファイル、欠落、サイズ・ハッシュの変化、必須の権利文書の欠落、外部権利確認記録の改変、用途の不一致を拒否する。PE32+のx64形式と証明書テーブルを検査し、自己署名を含む埋め込み署名があれば未署名方針との不一致で停止する。これは信頼済みの記録に対する整合確認であり、署名による作者の認証ではない。形式1〜4の古い検証ZIPは形式5として受理せず、現在のソースから作り直す。

## 公開候補の作成

公開時は、検査したソースだけを履歴のない新規公開リポジトリへ展開する。旧開発リポジトリの履歴やコミットから作ったZIPは転用しない。新しい公開先のクリーンなチェックアウトから、GitHub CLIの`gh`も用意して実行する。

```powershell
just audit
just package --public-candidate
just verify-release --public-candidate --archive dist/<候補名>/<候補名>.zip --report work/<候補名>-verification.json
```

作成時と最終検証時にoriginが確定済みの公開先を指し、GitHub API上でそのリポジトリが公開状態であり、HEADと同じコミットが存在することを照合する。所有者名・リポジトリ名の英字の大文字と小文字は区別せず、別の公開先や異なるコミットは受理しない。公開候補は`release`構成・変更なしに限定し、`--allow-dirty`との併用を拒否する。通常の`just package`は従来どおり検証用途である。展開済み公開候補の照合にも`cargo xtask package --verify <展開先> --public-candidate`を指定する。

Windows 11での`verify-release`は隣接する`SHA256SUMS.txt`でZIPと外部マニフェストを照合した後、ZIPを専用領域へ展開し、外部記録・文書・パス・同梱DLL・GUI・CLIを検証する。外部マニフェストに記録した権利確認記録のハッシュ、HTMLに掲載した原文と依存ごとの参照、提供ソースのハッシュ、版、コミット、ソース側Cargo.lock、追跡ファイル一覧のハッシュを照合する。自動検証は実Explorerの受け入れを代替しない。

通常CIとドラフト候補の生成では`--skip-shell`を指定する。ZIP・権利表示・文書・個人パス・GUI起動・CLIの検証を維持し、実Shellを使う試験ビルドとDLLの呼び出しを省略する。報告書の`shell_sta_mta`は`not-run`、`probe_sha256`は`null`となる。公開前には、対応対象のWindows 11 x64の通常権限で、同じ候補ZIPへ`--skip-shell`を付けずに再実行する。ZIPのハッシュが同じで`shell_sta_mta=passed`となった報告書と、[実Explorerの受け入れ](explorer-acceptance.ja.md)を揃えて公開を判断する。

公開候補の報告書にはZIPのファイル名とハッシュ、公開コミット、版、用途、未署名状態、検証結果を記録し、ローカルの絶対パスや生ログの場所を含めない。生ログは報告書に隣接する専用フォルダーへ保持し、公開には添付しない。検証用の報告書は調査のため従来のローカルパスを保持する。ZIP・チェックサム・公開候補の報告書を同じ成果物として扱い、検証後に再ビルドやファイル変更を行わない。

## 未署名の案内

SmartScreenの警告や、Smart App Control・組織の実行制御による起動制限があり得る。日英の[初回公開案内](releases/0.1.0.ja.md)で未署名を明示する。利用者へ自己署名証明書の信頼登録や保護機能の広範な無効化を求めない。SHA-256と、別途整備するGitHubのビルド来歴証明はWindowsのコード署名とは別の確認手段である。署名サービスや署名鍵を初回公開のために用意しない。

配布物には競合復旧と対象選択の案内を含める。分類表と言語定義はアプリへ組み込み、VS Code由来の言語定義の利用条件の原文を同梱する。開発用文書一式は収録しない。

`just verify-release --report work/release-doc-links-<SHA>.json` は、新しい最適化ZIPを作り、展開後の実ファイルに対して本体のMarkdown文書の相対リンクも検査する。対象は配布ルート直下と `docs/` 配下の案内文書であり、原文を保持する第三者の権利表示・同梱ソースの文書は対象にしない。インラインリンク、画像参照、参照形式のリンク定義を扱い、コード例・Web等の外部URL・同じ文書内の節参照を除外する。別文書の節付きリンクはファイルの存在を検査し、節名の妥当性や外部サイトの到達性は検査しない。配布物の外を指す相対パスやディレクトリへの参照も拒否する。

同じ検証でmanifest・ハッシュ、同梱DLLの隔離したSTA/MTA試験、GUIの起動・正常終了、CLIのJSON出力を確認する。報告書の `document_links` と `document_links_checked` に結果と照合件数を記録する。既存ZIPは上書きせず、変更のないコミットごとに新しい配布物と報告書を作る。

## 待機期限と失敗時のログ

配布ビルドと検証用プロセスには `xtask/src/process.rs` のWindowsジョブと待機期限を使う。失敗時も自身が起動した子孫だけを回収し、段階名、PID、経過時間、標準出力・標準エラーを残す。報告書は既存ファイルを上書きしない。

`cargo xtask verify-release` は報告書の隣へ一意なログフォルダーを作り、展開した一時領域を除去した後もログを保持する。成功時の `process_logs` はその場所を示す。失敗・期限超過時には成功報告書を作らず、例外の診断と同じログを調べる。GUI起動確認も30秒の期限超過時に途中ログを保持する。各段階の上限とRust側の試験については[開発と検証](development.ja.md#試験プロセスの待機期限と診断)を参照する。

## ビルド元のパスの除去

配布用のビルドでは、チェックアウト、Cargoの保存先、Rustのツールチェーンとユーザーフォルダーの絶対パスを、`/build/source`、`/build/cargo`、`/build/toolchain`等の中立な接頭辞へ置換する。Rustの`--remap-path-prefix`は文字列の置換であるため、Windowsの両方の区切りを指定する。開発者個別のパスは設定ファイルに固定せず、実行環境から求める。通常の`just run`やデバッグ用のビルドは変更しない。

配布ビルドの`CARGO_ENCODED_RUSTFLAGS`に置換指定と`-Cstrip=debuginfo`を渡す。環境変数の既存の追加引数は保持する。Cargoの設定ファイルにある追加のrustflagsより、この環境変数が優先される。ビルドスクリプトやネイティブのリンカーが埋め込むパスはRustの置換だけでは保証できないため、完成したEXE・DLLをUTF-8とUTF-16LEで検査する。ビルド元の接頭辞やWindowsのユーザーディレクトリが残る場合はZIPの生成を拒否する。PDBは同梱しない。

単独検査には`just check-binaries --folder <EXE・DLLのフォルダー>`を使う。`cargo xtask package --verify <展開先>`と`just verify-release`でも、ファイルのハッシュに加えて同じ検査を実行する。パス検査の成功は、コード署名、再現可能ビルド、すべての秘密情報の不在を保証するものではない。権利文書内の作者名・出典・著作権表示はこの除去処理の対象にしない。

## 権利表示の収集範囲

WindowsターゲットのCargo依存グラフから、GUI・CLI・Shell DLLが使う通常依存とビルド依存を再帰的に収集する。開発専用の依存辺は除外する。共有や機能統合により実バイナリへ入らない部品を含む可能性はあり、リンク済みコードだけを特定した一覧ではない。

各パッケージのライセンス・通知・著作権ファイルを配下も含めて収集し、原文を`licenses/THIRD_PARTY_LICENSES.html`に掲載する。完全に同じバイト列の原文は一度だけ掲載し、各依存から元の名前で参照する。異なる著作権表示や通知は省略せず、HTMLを展開した後の内容と収集時のSHA-256を照合する。UTF-8でない原文は変換して推測せず、生成を停止して確認する。パッケージに欠ける表示は、`licenses/upstream/manifest.json` が記録する上流コミットの原文を補完し、内容のSHA-256を照合する。複数許諾のうち一つだけを補完した場合は、元の宣言と補完原文を両方残す。Rust標準ライブラリは、使用するツールチェーンの `COPYRIGHT-library.html` も収録する。VS Codeの言語定義のMIT原文も`licenses/`に収録する。MPL-2.0依存のoption-extは、版を固定したcrates.ioの原本取得先とSHA-256をHTMLに示し、Cargo.lockと照合した未改変の原本をZIP外の`sources/`にも保持する。変更した場合は変更後のソースを提供する必要がある。

依存の不足と補完元は、ZIP外の `dependencies.json`、`manifest.json` の `license_review_required` と `licenses/upstream/manifest.json` で確認する。メタデータの宣言だけから許諾文を作らない。原文の対応確認に使ったsimd_helpersとseahashの照合資料は `docs/evidence/` に残し、通常の試験ログと区別する。

配布時はcargo-about 0.9.2を`--locked --offline --fail`で実行し、`about.toml`の許可・優先順位から選択する。閲覧用の`licenses/THIRD_PARTY_LICENSES.html`と、ZIP外に選択した識別子・本文・使用する依存の一覧を持つ`about.json`を生成する。Cargo.lockと設定のSHA-256も記録する。ツールの版、生成、補完元の照合に失敗した場合は配布を停止する。個人パスと開発補助専用の依存情報を配布用の出力へ持ち込まない。原文収集の範囲にあってcargo-aboutの通常ビルド対象にない依存は、原文と`selection_status=supplement-only`を保持する。

mac 0.1.1には、Apache Software Foundationの正規のApache-2.0全文、取り込み元のSimon SapinのMIT全文、html5everのMIT・COPYRIGHT・AUTHORSを同梱する。原本のcrateはCargo.lockと照合してZIP外に保持し、取得先とSHA-256をHTMLに示す。選択根拠と採用判断は[SELECTION.md](../licenses/upstream/mac-0.1.1/SELECTION.md)、原文と出典の照合結果は[provenance.json](../licenses/upstream/mac-0.1.1/provenance.json)を正本とし、配布物の権利表示HTMLから参照できる。

公開時にはZIP外の`sources/`の原本をリリースの別添として保管し、権利表示HTMLに記載した取得先が利用できなくなった場合も、対象ソースを合理的な方法で提供できるよう維持する。検証時はHTMLのURLとハッシュ、保存した原本のハッシュを照合するが、ネットワーク上の取得先の継続的な可用性は保証しない。

2026-10-25、FileHueの管理者は現行の補完内容と残る不確実性を承知し、この版の配布を自己責任で採用した。`licenses/upstream/manifest.json`の`acceptance`は判断者・日付・理由・原本crateのハッシュ・補完一覧のハッシュを記録する。一覧は同じパッケージと版の補完レコードを元の順で並べ、`acceptance`を除き、オブジェクトのキーを辞書順とした空白なしのUTF-8 JSONからSHA-256を求める。生成時に全補完ファイルと原本を照合したうえで、この組み合わせだけ`review_required=false`にする。判断と上流未確認の事実はdependencies.jsonと閲覧用HTMLにも残す。他の不足や将来版には流用しない。

mac固有の権利表示・追加通知の完全性が上流で確認されたわけではなく、汎用本文を上流が発行したLICENSEの復元と表現しない。新しい原文・回答・利用条件の変更が判明した場合は判断を見直す。

## 公開用配布へ移る条件

通常は`purpose=private-validation-only`、明示指定した公開候補は`purpose=public-candidate`とする。両方で`publication_approved=false`を維持し、生成・検証の成功を人による公開許可として記録しない。未解決の権利確認が残れば公開候補の作成を停止する。実Explorerの[受け入れ手順](explorer-acceptance.ja.md)と明示的な公開許可を別途確認し、同じ最終ZIPを公開まで保持する。公開先でのCI・来歴証明・リリース添付は[配布候補の生成と公開](ci/public-release.ja.md)を参照する。
