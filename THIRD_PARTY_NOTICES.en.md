# Third-party notices

[日本語](THIRD_PARTY_NOTICES.md) | English

## VS Code language mappings

`crates/icon-core/data/languages.json` contains language identifiers, extensions, filenames, and filename patterns extracted from the language definitions in Microsoft's public VS Code repository.

- Source: [Microsoft / VS Code](https://github.com/microsoft/vscode).
- The `commit` in the mapping identifies the source revision. When regenerating it, obtain the license text from the same revision and record it in `license_sha256`.
- Individual source URLs are recorded in `sources` in the mapping.
- Copyright and license: VS Code's MIT license, included in the source as `crates/icon-core/data/VSCODE-LICENSE.txt` and in the distribution as `licenses/VSCODE-LICENSE.txt`.
- Regeneration: `scripts/update-language-catalog.ps1`.

The additional TOML, YAML, and Zig mappings are independent rules based on this project's initial requirements; they are not part of the generated mapping above.

## External icon packs and Rust dependencies

Icon-pack artwork is not bundled in this repository. On import, the application preserves the pack's declarations, licenses, notices, and README files, including those in subdirectories or explicitly referenced, in the cached `provenance.json`. For officially downloaded packs, it also records the archive's SHA-256, download URL, version, license summary, and conversion to ICO. The artwork retains its original terms and is not relicensed under the application's MIT license. See the Japanese [pack download design](docs/pack-downloads.ja.md) for the reviewed packs and their terms.

The full list of Rust dependencies and distribution notices is assembled from Cargo.lock and the components selected for distribution. This document alone is not a complete notice for every dependency.

The distribution includes `licenses/THIRD_PARTY_LICENSES.html`, which contains complete license texts, original copyright notices and other notices, and links to source code. Run `just licenses` from the source to generate the same format. The generated notices combine cargo-about's license selections with the original rights documents. The Japanese `docs/distribution.ja.md` describes the generation process.

For mac 0.1.1, the project selects Apache-2.0 and also retains the MIT terms and copyright notices from the imported code. On 2026-10-25, the FileHue maintainer accepted distribution of this version with the current supplementary documents while acknowledging that upstream has not confirmed all mac-specific notices. `SELECTION.md` and `provenance.json` document the rationale, decision, pinned sources, and remaining uncertainty. They are in `licenses/upstream/mac-0.1.1/` in the source and can be accessed through the distribution's third-party license HTML.
