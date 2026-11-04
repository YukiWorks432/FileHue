# 配布候補の生成と公開

[Windows検証](../../.github/workflows/windows.yml)はPRとmainへの変更を読み取り権限で検証する。[配布候補の作成](../../.github/workflows/public-release.yml)は、確定した公開コミットから同じZIPの検証・ビルド来歴証明・ドラフト添付を行う。実Explorerでの受け入れとリリース公開は、その後に確認する。

## 公開先の設定

1. 公開先を`YukiWorks432/FileHue`、基点を`main`とする。Rust、検証ツール、Actionはワークフローに記載した固定版を使う。
2. リポジトリの設定で[変更不可のリリース](https://docs.github.com/en/code-security/concepts/supply-chain-security/immutable-releases)を有効にする。公開済みの添付物やタグは差し替えず、修正には新しい版を使う。
3. [配布手順](../distribution.ja.md)に従って依存・権利を確認し、承認した対象コミットへ版に一致するタグを作成する。ワークフローは新しいタグを作らない。

PR検証は署名・公開権限を持たない。配布候補のビルド側には読み取りと来歴証明、ドラフト添付側だけにリリースの書き込み権限を与える。署名鍵や署名サービスの資格情報は使わない。

## 候補の作成と確認

公開先の`main`から「公開候補の作成」を手動実行し、作成済みタグ（例`v0.1.0`）を入力する。タグ、Cargo.tomlの版、対象コミットが一致しない場合はビルドへ進まない。

`just verify`、文書、公開候補の生成と`just verify-release --skip-shell`が成功すると、ZIPの来歴を証明する。別ジョブが成果物と来歴を確認し、リリースをドラフトとして作る。失敗した場合はログと途中のドラフトを確認し、不足したまま公開しない。検証後にZIPを再ビルドしない。

Windows検証と候補作成は、Rust 1.99.0、just 1.43.0、cargo-deny 0.20.2、cargo-about 0.9.2を使う。検証ツールはチェックアウトしたソースの外で導入し、保存先を公式のキャッシュActionで再利用する。

Windows検証は、通常検証と最適化ZIPの生成・検証を別のWindows環境で並列に実行する。両方が成功した場合だけ、集約した「WindowsのビルドとGPUI操作試験」を成功とする。通常検証は`just verify`、ZIP検証は`just audit`によって、それぞれ製品のビルド前に最新の依存監査を行う。

通常CIでは、Cargoの`registry`と`target`を構成別にキャッシュする。キーにはRust版、通常検証／ZIP検証の区別、Cargoの依存定義・設定、CI設定、配布ビルドのパス置換処理のハッシュと対象コミットを含める。同じ構成と設定の過去のキャッシュを復元し、現在のソースとの差分はCargoが再構築する。mainへのプッシュで検証に成功したジョブだけが保存し、PRと手動実行は復元のみとする。監査データベース、資格情報、`work/`、配布ZIPはこのキャッシュに含めない。

公開候補の作成では、従来どおり検証ツールだけをキャッシュする。通常CIの製品・依存のビルド結果を復元せず、確定した公開コミットから再ビルドする。配布用のパス置換、権利表示と同じ完成ZIPの検証を維持する。

初回のEXEとDLLは未署名である。SHA-256は内容の一致を、[ビルド来歴証明](https://docs.github.com/en/actions/how-tos/secure-your-work/use-artifact-attestations/use-artifact-attestations)は作成元を確認するもので、Windowsのコード署名ではない。ZIP、`SHA256SUMS.txt`、外部マニフェスト、提供ソース、検証報告が同じ版・コミットを示すことを確認し、必要な原本を継続して提供できるよう保持する。

通常CIはメモリ上のレジストリ実装やCOMの試験用実装と、実際の本体ロジックを組み合わせて検証する。実Shell試験をGitHub管理ランナーで実行せず、一時ユーザーの作成や権限変更を行わない。CI報告書の`shell_sta_mta=not-run`は未実施を表し、公開の受け入れ結果には使わない。

対応対象のWindows 11 x64の通常権限の環境で、対象コミットのソースとドラフトから取得した同じZIPを用意し、`just verify-release --public-candidate --archive <候補ZIP> --report <新しい報告書>`を実行する。ZIPのハッシュがCI報告と一致し、`shell_sta_mta=passed`であることを確認する。Shellの登録・保護・委譲・復元を変更した場合も、[実Shell検証](../development.ja.md#windows-11での実shell検証)を実行する。

同じWindows 11環境で、ハッシュと`gh attestation verify <ZIP> --repo YukiWorks432/FileHue`を確認する。[実Explorerの受け入れ](../explorer-acceptance.ja.md)と、起動、権利表示、更新・復元・削除を確認し、結果をGUI起動試験や隔離DLL試験と区別して残す。

担当者がタグ・版・コミット、添付物、受け入れ結果を照合し、日英の公開案内を揃えて公開を判断する。ドラフト作成はリリースを公開しない。公開後の修正は次の版で行う。
