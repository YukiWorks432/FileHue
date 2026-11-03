# 配布候補の生成と公開

[Windows検証](../../.github/workflows/windows.yml)はPRとmainへの変更を読み取り権限で検証する。[配布候補の作成](../../.github/workflows/public-release.yml)は、確定した公開コミットから同じZIPの検証・ビルド来歴証明・ドラフト添付を行う。実Explorerでの受け入れとリリース公開は、その後に確認する。

## 公開先の設定

1. 公開先を`YukiWorks432/FileHue`、基点を`main`とする。Rust、検証ツール、Actionはワークフローに記載した固定版を使う。
2. リポジトリの設定で[変更不可のリリース](https://docs.github.com/en/code-security/concepts/supply-chain-security/immutable-releases)を有効にする。公開済みの添付物やタグは差し替えず、修正には新しい版を使う。
3. [配布手順](../distribution.ja.md)に従って依存・権利を確認し、承認した対象コミットへ版に一致するタグを作成する。ワークフローは新しいタグを作らない。

PR検証は署名・公開権限を持たない。配布候補のビルド側には読み取りと来歴証明、ドラフト添付側だけにリリースの書き込み権限を与える。署名鍵や署名サービスの資格情報は使わない。

## 候補の作成と確認

公開先の`main`から「公開候補の作成」を手動実行し、作成済みタグ（例`v0.1.0`）を入力する。タグ、Cargo.tomlの版、対象コミットが一致しない場合はビルドへ進まない。

`just verify`、文書、公開候補の生成と完成ZIPの検証が成功すると、ZIPの来歴を証明する。別ジョブが成果物と来歴を確認し、リリースをドラフトとして作る。失敗した場合はログと途中のドラフトを確認し、不足したまま公開しない。検証後にZIPを再ビルドしない。

Windows検証と候補作成は、Rust 1.99.0、just 1.43.0、cargo-deny 0.20.2、cargo-about 0.9.2を使う。検証ツールの保存先だけを公式のキャッシュActionで再利用し、製品のビルド出力や資格情報はキャッシュに含めない。ツールの導入はチェックアウトしたソースの外で行う。

初回のEXEとDLLは未署名である。SHA-256は内容の一致を、[ビルド来歴証明](https://docs.github.com/en/actions/how-tos/secure-your-work/use-artifact-attestations/use-artifact-attestations)は作成元を確認するもので、Windowsのコード署名ではない。ZIP、`SHA256SUMS.txt`、外部マニフェスト、提供ソース、検証報告が同じ版・コミットを示すことを確認し、必要な原本を継続して提供できるよう保持する。

新しいWindows環境でドラフトから同じZIPを取得し、ハッシュと`gh attestation verify <ZIP> --repo YukiWorks432/FileHue`を確認する。[実Explorerの受け入れ](../explorer-acceptance.ja.md)と、起動、権利表示、更新・復元・削除を確認し、結果をGUI起動試験や隔離DLL試験と区別して残す。

担当者がタグ・版・コミット、添付物、受け入れ結果を照合し、日英の公開案内を揃えて公開を判断する。ドラフト作成はリリースを公開しない。公開後の修正は次の版で行う。
