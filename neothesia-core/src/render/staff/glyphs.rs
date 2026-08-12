//! Cached SMuFL (Bravura) glyph buffers for the staff renderer.
//!
//! Buffers are built once at a given font size and reused across frames (see
//! `note_labels::LabelsCache` for the same pattern). They are built without a color attribute so
//! the per-area `default_color` (white / gray) drives the color, letting one buffer serve both.

use std::collections::HashMap;

use glyphon::{self, Buffer};

use crate::render::TextRenderer;

/// Bravura (SMuFL) codepoints.
pub mod cp {
    pub const G_CLEF: char = '\u{E050}';
    pub const F_CLEF: char = '\u{E062}';
    pub const TIME_SIG_0: char = '\u{E080}';
    pub const NOTEHEAD_WHOLE: char = '\u{E0A2}';
    pub const NOTEHEAD_HALF: char = '\u{E0A3}';
    pub const NOTEHEAD_BLACK: char = '\u{E0A4}';
    pub const FLAT: char = '\u{E260}';
    #[allow(dead_code)]
    pub const NATURAL: char = '\u{E261}';
    pub const SHARP: char = '\u{E262}';
    // up codes: 8th E240, 16th E242, 32nd E244; down = up + 1
    pub fn flag(count: u8, down: bool) -> char {
        let up = match count {
            1 => '\u{E240}',
            2 => '\u{E242}',
            _ => '\u{E244}',
        };
        if down {
            char::from_u32(up as u32 + 1).unwrap_or(up)
        } else {
            up
        }
    }

    /// Is `ch` one of the flag glyphs (E240..E245)? Flags are anchored by their true ink box (see
    /// [`GlyphCache::build`]), not the em/advance box, because the flag's ink is a small sliver
    /// offset far inside the em box — anchoring by the em box leaves the flag floating off the
    /// stem.
    pub fn is_flag(ch: char) -> bool {
        matches!(ch, '\u{E240}'..='\u{E245}')
    }

    /// Down-stem flags are the odd codepoints (up + 1).
    pub fn is_flag_down(ch: char) -> bool {
        is_flag(ch) && (ch as u32) & 1 == 1
    }
}

/// Cached glyph: the shaped buffer, its typographic (advance width, line height) in logical px,
/// and the glyph's true ink bounding box `InkRect` (logical px, relative to the buffer top-left).
///
/// `measure()` returns the advance/em box, which for SMuFL flags is several times larger than the
/// actual ink and would leave a flag floating off the stem if used for anchoring. Flags use the
/// ink box instead; other glyphs still centre-anchor on the em box.
#[derive(Clone, Copy, Default)]
pub struct InkRect {
    /// Ink left edge, relative to the buffer's left.
    pub x: f32,
    /// Ink top edge, relative to the buffer's top.
    pub y: f32,
    /// Ink width.
    pub w: f32,
    /// Ink height.
    pub h: f32,
}

/// Cached glyph: `(buffer, advance_width, line_height, ink_box)`.
pub type Cached = (Buffer, f32, f32, InkRect);

pub struct GlyphCache {
    size: f32,
    map: HashMap<char, Cached>,
}

impl GlyphCache {
    pub fn new() -> Self {
        Self {
            size: 0.0,
            map: HashMap::new(),
        }
    }

    fn build(ch: char, size: f32) -> Cached {
        let font_system = crate::font_system::font_system();
        let mut font_system = font_system.borrow_mut();

        let mut buffer = Buffer::new(&mut font_system, glyphon::Metrics::new(size, size));
        buffer.set_size(Some(f32::MAX), Some(f32::MAX));
        buffer.set_wrap(glyphon::Wrap::None);
        buffer.set_text(
            ch.to_string().as_str(),
            &glyphon::Attrs::new().family(glyphon::Family::Name("Bravura")),
            glyphon::Shaping::Basic,
            None,
        );
        buffer.shape_until_scroll(&mut font_system, false);

        let (w, h) = TextRenderer::measure(&buffer);

        // True ink bounding box of the single glyph, in logical px relative to the buffer's
        // top-left. `measure()` returns the typographic advance/em box, which for SMuFL flags is
        // far larger than the actual ink — anchoring a flag by it leaves the fin floating off the
        // stem. Rasterize once at scale 1.0 (logical == physical px) via swash and read the
        // placement (ink bearing + size) instead.
        let ink = {
            // glyphon renders a glyph image at `y = line_y*scale + TextArea.top - placement.top`
            // (see glyphon text_render.rs). So the ink's vertical offset from `TextArea.top` is
            // `line_y + pen_y - placement.top` — NOT `pen_y + placement.top`. The two only agree
            // when `placement.top` is ~0; for SMuFL bass flags `placement.top` is ~36, and the
            // sign difference left the bass flag floating ~50px too high. Capture the run's
            // `line_y` (the baseline) alongside the glyph to compute the true offset.
            let run_info = buffer.layout_runs().next().and_then(|run| {
                let g = run.glyphs.first()?;
                Some((
                    run.line_y,
                    g.physical((0.0, 0.0), 1.0).cache_key,
                    g.x + g.font_size * g.x_offset,
                    g.y - g.font_size * g.y_offset,
                ))
            });
            match run_info {
                Some((line_y, key, pen_x, pen_y)) => {
                    let mut swash = glyphon::SwashCache::new();
                    swash
                        .get_image_uncached(&mut font_system, key)
                        .map(|img| InkRect {
                            x: pen_x + img.placement.left as f32,
                            y: line_y + pen_y - img.placement.top as f32,
                            w: img.placement.width as f32,
                            h: img.placement.height as f32,
                        })
                        .unwrap_or_default()
                }
                None => InkRect::default(),
            }
        };

        (buffer, w, h, ink)
    }

    /// Ensure every glyph in `chars` is built at `size` (rebuilding the whole cache if the size
    /// changed). Call this before [`GlyphCache::get_ref`] so the immutable lookups never miss.
    pub fn ensure(&mut self, chars: impl IntoIterator<Item = char>, size: f32) {
        if self.size != size {
            self.map.clear();
            self.size = size;
        }
        let to_build: Vec<char> = chars
            .into_iter()
            .filter(|c| !self.map.contains_key(c))
            .collect();
        for ch in to_build {
            self.map.insert(ch, Self::build(ch, size));
        }
    }

    /// Immutable lookup of an already-primed glyph (call [`GlyphCache::ensure`] first).
    pub fn get_ref(&self, ch: char) -> &Cached {
        self.map
            .get(&ch)
            .expect("staff glyph used before GlyphCache::ensure")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression check: the flag ink boxes must be measured by glyphon's real render formula
    /// (`line_y - placement.top`), so up- and down-flags attach at opposite stem ends. If the
    /// up- and down-flag ink bottoms ever match, the old `pen_y + placement.top` sign bug is back.
    #[test]
    fn flag_ink_boxes() {
        let mut cache = GlyphCache::new();
        let size = 4.0 * 10.0; // matches FONT_SIZE in mod.rs
        let cases = [
            ('\u{E240}', "flag8thUp  "),
            ('\u{E241}', "flag8thDown"),
            ('\u{E242}', "flag16thUp "),
            ('\u{E243}', "flag16thDn "),
        ];
        cache.ensure(cases.iter().map(|(c, _)| *c), size);
        for (c, name) in cases {
            let (_, _advance_w, _line_h, ink) = cache.get_ref(c);
            eprintln!(
                "{name} U+{:04X}: ink x={:>7.2} y={:>7.2} w={:>6.2} h={:>6.2} | top={:>7.2} bottom={:>7.2}",
                c as u32, ink.x, ink.y, ink.w, ink.h, ink.y, ink.y + ink.h,
            );
        }
        // The down-flag's ink must straddle the TextArea origin (top < 0 < bottom), i.e. its
        // stem-attach (bottom edge) is just below the anchor — not 50px off like the old bug.
        let down = cache.get_ref('\u{E241}').3;
        assert!(down.y < 0.0, "down-flag ink top should be above the anchor");
        assert!(down.y + down.h > 0.0, "down-flag ink bottom should be below the anchor");
    }
}
