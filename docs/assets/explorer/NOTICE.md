# Explorer comparison illustrations / エクスプローラーの比較画像

READMEの比較画像と`source.html`は、架空のファイル一覧を使った再現図です。実際のエクスプローラーの撮影やAI画像生成は行っていません。画像にはvscode-icons 13.0.0の素材を含みます。本体のMITとは区別して、以下の条件を適用します。

These README images and `source.html` reconstruct Explorer with fictional files. They are not captures of a user's computer or AI-generated images. They include artwork from vscode-icons 13.0.0 under the terms below, separately from FileHue's MIT license.

## Attribution and terms

- Artwork: **vscode-icons 13.0.0**, by the **vscode-icons contributors**.
- Pinned source: [commit 866d4f48701ba71e39d309e9e519bcf2eaf1c3e2](https://github.com/vscode-icons/vscode-icons/tree/866d4f48701ba71e39d309e9e519bcf2eaf1c3e2).
- Original license statement: [upstream README, License section](https://github.com/vscode-icons/vscode-icons/blob/866d4f48701ba71e39d309e9e519bcf2eaf1c3e2/README.md#license).
- General artwork license: [Creative Commons Attribution-ShareAlike 4.0 International](https://creativecommons.org/licenses/by-sa/4.0/), [legal code](https://creativecommons.org/licenses/by-sa/4.0/legalcode).
- Branded icons retain the terms of their respective copyright owners, as stated upstream. These notices do not grant trademark rights or imply endorsement by those owners.
- Changes: the SVG artwork is unmodified; it is scaled and placed in an original Explorer illustration. FileHue's original visual contributions to the comparison images are licensed under CC BY-SA 4.0. This does not replace the underlying artwork's terms.
- FileHue's HTML/CSS/JavaScript rendering code is covered by the repository's MIT license. The embedded SVG artwork retains the terms above.

The upstream license statement says:

> The icons are licensed under the Creative Commons - ShareAlike (CC BY-SA) license.
>
> Branded icons are licensed under their copyright license.

The [provenance record](provenance.json) identifies the fixed VSIX, its SHA-256, each original SVG and its SHA-256, the filename and extension associations, and the two PNG hashes. The SVGs are embedded unchanged in [source.html](source.html); no extension programs or fonts are included.

## 再現方法 / Reproduction

`source.html?lang=ja`または`source.html?lang=en`をChromium系ブラウザで開き、アイコンとフォントの読み込み完了後に`main.sheet`要素を撮影します。CSS上の幅は1680px、出力倍率は2です。確認済みのPNGは3360 × 1766pxです。保存するファイル名は`comparison.ja.png`と`comparison.en.png`です。

Open `source.html?lang=ja` or `source.html?lang=en` in a Chromium browser. After images and fonts finish loading, capture `main.sheet` at a CSS viewport width of 1680px and a device scale factor of 2. The checked-in PNGs are 3360 × 1766px. Browser and system-font versions may affect rasterization. No network requests are needed to render this source.

`AGENTS.md`、`Cargo.toml`、`package.json`、`tsconfig.json`はファイル名で一致させています。通常の拡張子はFileHueの言語対応表でも解決しています。このパックでは`README.md`と`notes.md`は同じMarkdownアイコンです。フォルダー、ファイル名、種類、サイズは比較の左右で同一です。

The examples use exact filename matches for `AGENTS.md`, `Cargo.toml`, `package.json`, and `tsconfig.json`, plus FileHue's language mappings for extensions. `README.md` and `notes.md` use the same Markdown icon in this pack. Folders, names, types, and sizes are identical on both sides.
