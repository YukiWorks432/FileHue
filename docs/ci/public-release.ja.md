# 公開リポジトリでの配布候補

本手順と[設定例](public-release.yml.example)は、新しい公開リポジトリでの導入準備である。例は`.github/workflows/`の外に置き、非公開の開発リポジトリではActionsを実行しない。実行結果や公開許可を意味しない。

## 導入

1. 非公開の履歴・Issue・PR・作業ログを含めず、確認済みソースを新しい`YukiWorks432/filehue`へ展開する。公開先の設定と対象コミットを確認する。
2. [通常のWindows検証例](windows.yml.example)と配布候補の設定例をレビューし、公開先の`.github/workflows/`へ配置する。PR検証は読み取り権限とし、候補の作成と分ける。固定版のRust、依存、Actionと7日待機の方針を再確認する。
3. 公開先の設定で[変更不可のリリース](https://docs.github.com/en/code-security/concepts/supply-chain-security/immutable-releases)を有効にする。公開済みの添付物やタグは差し替えず、修正には新しい版を使う。
4. [配布手順](../distribution.ja.md)に従って依存・権利を確認し、対象コミットの版に一致するタグを公開先へ作成する。タグは対象コミットへ固定し、設定例は新規タグを自動作成しない。

## 候補の作成と確認

公開先の`main`から設定例を手動実行し、作成済みタグを入力する。ビルド側は読み取りと来歴証明の権限を使い、`just verify`、文書、公開候補の生成と完成ZIPの検証に成功した後、ZIPの来歴を証明する。別ジョブが成果物と来歴を確認し、リリースをドラフトとして作る。失敗したら途中のドラフトを確認し、不足したまま公開しない。

初回のEXEとDLLは未署名である。SHA-256は内容の一致を、[ビルド来歴証明](https://docs.github.com/en/actions/how-tos/secure-your-work/use-artifact-attestations/use-artifact-attestations)は作成元を確認するもので、Windowsのコード署名ではない。ZIPと`SHA256SUMS.txt`、外部マニフェスト、権利表示、提供ソースと検証報告が同じ版・コミットを示すことを確認する。必要な原本は継続して提供できるよう保持する。

新しいWindows環境でドラフトから同じZIPを取得し、ハッシュと`gh attestation verify <ZIP> --repo YukiWorks432/filehue`を確認する。[実Explorerの受け入れ](../explorer-acceptance.ja.md)と、起動、権利表示、更新・復元・削除も確認し、結果をGUI起動試験や隔離DLL試験と区別する。個人パスや非公開の記録を公開資料へ含めない。

担当者がタグ・版・コミット、すべての添付物、上記の結果を照合し、公開を判断する。ドラフト作成は公開を行わない。公開後に問題が見つかった場合は次の版で修正する。
