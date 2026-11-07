# arkhamicons

`arkhamicons.ttf` is the icon font of [ArkhamCards](https://github.com/zzorba/ArkhamCards): one glyph per game symbol (skill icons, action and reaction, chaos tokens, class symbols and the rest). The web client draws card text's `[combat]`, `[elder_thing]` and the like with it. `src/glyph.rs` maps each markup token to its codepoint, and `style.css` loads the font with `@font-face`. Trunk copies it into `dist/` beside the stylesheet (`index.html`, `rel="copy-file"`).

## Source

- **Repository:** https://github.com/zzorba/ArkhamCards
- **Commit:** `5d805697d5c7359fc6baffa3be90d1158ef7f33d`
- **File:** `assets/arkhamicons.ttf` at that commit, downloaded from `https://raw.githubusercontent.com/zzorba/ArkhamCards/5d805697d5c7359fc6baffa3be90d1158ef7f33d/assets/arkhamicons.ttf`
- **SHA-256:** `6acabe2503ff4ce06586aabb647cda764d29d444628b6cdd8c353ded93cc6ac4`

The file is vendored **unmodified**: no subsetting, no conversion to `.woff2`.

## Codepoints

Every codepoint in `src/glyph.rs`'s `GLYPHS` was checked against the same commit's `assets/arkhamicons-config.json` (the IcoMoon project the font is built from): each icon's `properties.code` is its codepoint and `properties.name` its token. All 23 names the table uses (`fast` is not one; it shares `free`'s glyph) carry the codepoint the table gives them. A refresh re-runs that check before replacing the file.

## Licence

ArkhamCards' repository carries no licence, so the font is all rights reserved. Bundling it is a deliberate exception to "Never re-host card art", recorded with its reasons in [`docs/product-decisions.md`](../../../../docs/product-decisions.md) → Legal posture.
