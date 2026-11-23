# The atlas's icon faces

The atlas bundles two faces, both subset to exactly the characters its interface
draws. Both are appended to both font families as their last *configured*
fallbacks (see `install_icon_fonts` in `src/viewer.rs`) — a fallback, never the
primary face: egui measures rows and baselines from the primary font, so
Ubuntu-Light must stay in front, and installed fonts are still consulted before
the system's font provider.

## `OpenMoji-color-subset.ttf` — the picture face

A 38 KB subset of **OpenMoji Color 17.0**, built from the official
`OpenMoji-color-glyf_colr_0` font (COLRv0 + CPAL: flat vector layers, no
bitmaps).

The viewer writes its pictograms as the emoji codepoints of its own lines — and of
all six translations of them:

```rust
panel_title(ui, self.t("🌍 Atlas Viewer"));
card_title(ui, self.t("🧊 Polar Ice Wasteland"));
```

egui ships no emoji face of its own, so without this font every one of those
codepoints is drawn by whatever the desktop's font fallback happens to pick —
Symbola for one glyph, Noto Emoji for the next, Ubuntu-Light for `⚠` — which is
why the atlas used to mix drawing styles. One bundled face gives every pictogram
the same flat, outlined drawing on every platform.

epaint only paints colour glyphs with its `color_fonts` feature enabled, which
`Cargo.toml` turns on. Colour glyphs keep their own palette; monochrome text is
still tinted by the theme as usual.

## `AtlasMarks-subset.ttf` — the mark face

A 7 KB subset of **Adwaita Mono** holding the three symbols that are typography
rather than pictures:

| Mark | Codepoint | Where |
| :--- | :--- | :--- |
| `☰` | U+2630 | the Genesis-panel toggle |
| `↺` | U+21BA | reset a setting / re-grow an act |
| `→` | U+2192 | “Province → Area” in tooltips |

They must not be painted as emoji, but they must not be borrowed from the desktop
either, or the atlas would look different on every machine. A monochrome face
keeps one clean stroke for all three and lets the theme tint them like the text
around them.

## What is inside the picture face

42 codepoints: every character in `src/viewer.rs` and `src/viewer/*.rs` that
falls in the emoji/symbol blocks (`U+1F000–1FAFF`, `U+2600–27BF`, `U+2B00–2BFF`,
`U+2300–23FF`, `U+25A0–25FF`), minus the three marks above. 41 are COLR base
glyphs; the 42nd is `U+FE0F`, the emoji presentation selector used by `🏔️` and
`⛰️` (zero advance, included so those sequences lay out cleanly). The wheel of
the Religions scope, `🛐` (`U+1F6D0`), is one of the COLR glyphs.

The test `every_icon_comes_from_a_bundled_face` in `src/viewer.rs` reads those
sources and fails if a pictogram is not resolved by the picture face, or one of
the three marks by the mark face — a new glyph cannot silently fall back to the
desktop's own fonts.

## Regenerating the subsets

Add the new glyph anywhere in the viewer first, then rebuild both faces from the
official fonts (fontTools 4.x is required):

```sh
curl -L -o /tmp/OpenMoji-color-glyf_colr_0.ttf \
  https://cdn.jsdelivr.net/gh/hfg-gmuend/openmoji@17.0.0/font/OpenMoji-color-glyf_colr_0/OpenMoji-color-glyf_colr_0.ttf

python3 - <<'EOF'
import glob
from fontTools.ttLib import TTFont
from fontTools import subset

# ── The picture face ────────────────────────────────────────────────────────
# The blocks the atlas draws pictures from, and the marks that are typography.
RANGES = [(0x1F000, 0x1FAFF), (0x2600, 0x27BF), (0x2B00, 0x2BFF),
          (0x2300, 0x23FF), (0x25A0, 0x25FF)]
TEXT_MARKS = set('\u2630\u21ba\u2192')  # ☰ ↺ →

want = set()
for path in ['src/viewer.rs'] + glob.glob('src/viewer/*.rs'):
    for c in open(path, encoding='utf-8').read():
        cp = ord(c)
        if c not in TEXT_MARKS and any(lo <= cp <= hi for lo, hi in RANGES):
            want.add(cp)
want.add(0xFE0F)  # the emoji presentation selector

font = TTFont('/tmp/OpenMoji-color-glyf_colr_0.ttf')
opts = subset.Options()
opts.layout_features = ['*']
opts.name_IDs = ['*']
opts.notdef_outline = True
opts.recalc_bounds = True
sub = subset.Subsetter(options=opts)
sub.populate(unicodes=sorted(want))
sub.subset(font)
font.save('assets/fonts/OpenMoji-color-subset.ttf')

# ── The mark face ───────────────────────────────────────────────────────────
# Adwaita Mono (SIL OFL 1.1), as shipped by the `adwaita-mono-fonts` package or
# built from https://gitlab.gnome.org/GNOME/adwaita-fonts.
marks = TTFont('/usr/share/fonts/adwaita-mono-fonts/AdwaitaMono-Regular.ttf')
marks_opts = subset.Options()
marks_opts.layout_features = []
marks_opts.name_IDs = ['*']
marks_opts.notdef_outline = True
mark_sub = subset.Subsetter(options=marks_opts)
mark_sub.populate(unicodes=[0x2630, 0x21BA, 0x2192])  # ☰ ↺ →
mark_sub.subset(marks)
# The subset is a modified version, so it carries its own name (see the OFL).
for nid, value in [(1, 'Atlas Marks'), (3, 'Atlas Marks'), (4, 'Atlas Marks'),
                   (6, 'AtlasMarks-Regular'), (16, 'Atlas Marks')]:
    marks['name'].setName(value, nid, 3, 1, 0x409)
marks.save('assets/fonts/AtlasMarks-subset.ttf')
print('wrote', len(want), 'picture codepoints and 3 marks')
EOF
```

Run `cargo test every_icon_comes_from_a_bundled_face` afterwards; it is the check
that both subsets and the sources agree.

## Licence and attribution

**OpenMoji** is published under **Creative Commons Attribution-ShareAlike 4.0
International** (CC BY-SA 4.0) by the OpenMoji authors — <https://openmoji.org>.
The full licence text is in [`LICENSE-OpenMoji.txt`](LICENSE-OpenMoji.txt).

`OpenMoji-color-subset.ttf` is an adaptation: a subset of *OpenMoji Color 17.0*
(`OpenMoji-color-glyf_colr_0.ttf`) containing only the codepoints the atlas draws.
It is distributed under the same CC BY-SA 4.0 licence. The black outlines and
palette are unmodified; only unused glyphs (and the tables that index them) were
removed with `fontTools.subset`.

**Adwaita Mono** is part of the GNOME Adwaita Fonts (an Iosevka customized to
match Inter, © 2015-2026 Renzhi Li) and is published under the **SIL Open Font
License 1.1** — <https://gitlab.gnome.org/GNOME/adwaita-fonts>. The full licence
text is in [`LICENSE-AdwaitaMono.txt`](LICENSE-AdwaitaMono.txt).

`AtlasMarks-subset.ttf` is a modified version — a three-glyph subset — and
therefore carries its own family name (“Atlas Marks”), as the OFL requires. It is
distributed under the same OFL 1.1 licence.
