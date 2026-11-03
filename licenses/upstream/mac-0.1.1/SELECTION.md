# mac 0.1.1の選択根拠 / License selection

## 採用する条件

mac本体はApache-2.0を選択する。根拠は、0.1.1のCargo.tomlとREADMEにあるMIT／Apache-2.0の宣言、および作者が二重ライセンスへ変更したコミットである。APACHE-2.0.txtはApache Software Foundationが公開する正規本文であり、macの上流が発行したLICENSEファイルを復元したものではない。本文の付録にある年・権利者の記入例を、推測した値に置き換えない。

取り込み元の表示も保持する。SIMON-SAPIN-MIT.txtはmatches!の元実装の著作権表示を含むMIT全文であり、mac本体のApache-2.0選択だけで省略しない。HTML5EVER-MIT.txt、HTML5EVER-COPYRIGHT.txt、HTML5EVER-AUTHORS.txtはhtml5ever由来のマクロの原文である。これらをmacの作者自身の著作権表示として扱わない。crate内のCargo.tomlとREADMEも原文のまま同梱する。

mac本体の著作権表示原文と追加通知の完全性について、作者の確認は得られていない。権利表示の追加を求める上流Issue #14は未解決である。2026-10-25、FileHueの管理者はこの不確実性を承知し、現在の補完内容を保持したmac 0.1.1の配布を自己責任で採用すると決定した。これは上流による確認や法的保証を意味しない。

採用判断は原本crateのSHA-256と、補完7ファイルの出典・ハッシュ等を含む一覧のSHA-256に結び付ける。配布時は採用日・判断者・理由と上流未確認の事実を保持し、`acceptance`へ記録する。この組み合わせに限って`review_required=false`とし、別の版や補完内容へ自動適用しない。新しい原文・回答・利用条件の変更が判明した場合は見直す。

## Selected terms

Apache-2.0 is selected for mac itself based on the declarations in the 0.1.1 Cargo.toml and README and the author's dual-licensing commit. APACHE-2.0.txt is the official Apache Software Foundation text, not a recovered mac-specific LICENSE file. The appendix's placeholder copyright notice has not been filled with inferred dates or owners.

The original MIT notice for Simon Sapin's matches! implementation is retained separately. The html5ever MIT license, COPYRIGHT and AUTHORS files are also retained for the imported macros. These notices do not identify the copyright owners of mac's independently written portions. The crate's Cargo.toml and README are preserved unchanged.

The complete mac-specific copyright notice and any additional notices remain unconfirmed by upstream. Upstream issue #14 remains open. On 2026-10-25, the FileHue maintainer accepted distribution of mac 0.1.1 with the current supplements, taking responsibility for the remaining uncertainty. This decision is neither upstream confirmation nor a legal guarantee.

The acceptance is bound to the original crate checksum and a SHA-256 of the seven supplement records, including their sources and checksums. The date, decision maker, rationale, and upstream uncertainty are retained in the distribution records. Only this exact combination has `review_required=false` and an `acceptance` record. Other versions or changed supplements require a new decision. New source evidence, upstream responses, or changed terms require reconsideration.

## 固定した出典 / Pinned sources

- 配布原本のSHA-256: c41e0c4fef86961ac6d6f8a82609f55f31b05e4fce149ac5710e439df7619ba4
- [mac 0.1.1のソース](https://github.com/reem/rust-mac/tree/66afc663b68a65633ea165c742b5a9c6734581c2)
- [作者による二重ライセンス化](https://github.com/reem/rust-mac/commit/6b35d271ca8783a513aaa9bb47c2a926a68107d2)
- [html5everからの取り込み](https://github.com/reem/rust-mac/pull/5)
- [matches!の取り込みと元作者の回答](https://github.com/reem/rust-mac/pull/6#issuecomment-76515364)
- [未解決のライセンス追加要望](https://github.com/reem/rust-mac/issues/14)
- [Apache-2.0公式本文](https://www.apache.org/licenses/LICENSE-2.0.txt)

各原文の固定URLとSHA-256、crateとタグの全ファイルの照合結果は同梱のprovenance.jsonに記録する。確認日は2026-10-22である。
