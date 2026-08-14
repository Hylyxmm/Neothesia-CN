//! Scrolling grand-staff (五线谱) preview rendered between the progress bar and the waterfall.
//!
//! Geometry (staff lines, bar lines, stems, ledger lines, band background, play head) is drawn
//! with the shared [`QuadRenderer`]; musical glyphs (clefs, noteheads, accidentals, flags, time
//! signature) are drawn with the shared [`TextRenderer`] using the Bravura SMuFL font. Exactly
//! ~5 measures stay visible and scroll leftwards as playback advances (standard score direction);
//! the play head sits just right of a fixed clef block on the left. Noteheads are white until
//! their onset crosses the play head, then turn gray.

mod glyphs;
mod pitch;

use std::{collections::HashMap, sync::Arc, time::Duration};

use glyphon::{self, Color};

use crate::{
    render::{waterfall::NoteList, QuadInstance, QuadRenderer, TextRenderer},
    utils::{Point, Rect, Size},
};

use midi_file::MidiNote;

// ---- layout constants (logical px) -----------------------------------------
const STAFF_TOP_Y: f32 = 78.0;
const LINE_SPACING: f32 = 10.0; // distance between two adjacent staff lines
/// Noteheads are centred on their onset x, so a downbeat (onset exactly at the measure boundary)
/// would straddle its bar line — half the head lands in the previous measure. We don't move the
/// notes (that would break spacing/duration), so instead nudge each drawn bar line left by this
/// many notehead widths so the downbeat head sits to the right of the line. (The Bravura notehead
/// is a bit wider than one staff space, so this needs to be ≥~0.8 to open a visible gap.)
const BARLINE_NUDGE: f32 = 1.0 * LINE_SPACING;
/// Grace-note (倚音) rendering: a very short note (≤ 32nd) crushed just before a longer main note
/// is drawn smaller, just left of the main notehead, instead of at its literal (overlapping) time
/// position. GRACE_SCALE sizes the head/stem; GRACE_OFFSET is the centre-to-centre gap from the
/// main notehead.
const GRACE_SCALE: f32 = 0.6;
const GRACE_STEM_LEN: f32 = 2.0 * LINE_SPACING;
const GRACE_OFFSET: f32 = 1.5 * LINE_SPACING;
/// Ottava (8va/8vb/15ma/15mb) lines: a note that would need a 4th ledger line — beyond the 3 we
/// made room for (上加三线 / 下加三线) — is instead drawn transposed back into the staff with a
/// bracket marking the shift. `OTTAVA_LEDGER_MAX` is the most diatonic steps a note may sit from
/// the staff edge (top or bottom line) before an ottava kicks in: 7 = the space just past the 3rd
/// ledger line (still 3 lines, fits); 8 = the 4th ledger line (would clip → ottava).
const OTTAVA_LEDGER_MAX: i32 = 7;
const OTTAVA_LABEL_SIZE: f32 = 1.4 * LINE_SPACING; // "8va" text height
/// How many measures past the last visible column to pre-scan when detecting ottava runs, so a
/// run's bracket resolves to its true final note (off-screen) instead of growing as notes slide
/// into view. A few measures is enough: an ottava run rarely spans more than a couple of measures
/// ahead of the playhead.
const OTTAVA_LOOKAHEAD_MEASURES: usize = 4;
const TREBLE_TOP_OFFSET: f32 = 14.0;
// 2 line-spacings so middle C (C4) is a single shared ledger line between the two staves.
const INTER_STAFF_GAP: f32 = 2.0 * LINE_SPACING;
const CLEF_BLOCK_W: f32 = 80.0;
/// Fixed vertical extent of the staff band (top at `STAFF_TOP_Y`). Decoupled from
/// `LINE_SPACING` so shrinking the line spacing keeps the region (and its scissor) the same
/// size — the smaller staff is centred within it instead of being clipped top/bottom.
///
/// The band is also the scissor rect, so notes whose ledger lines reach above the treble staff
/// (上加线) or below the bass staff (下加线) are hard-clipped at the band edge. The staff system
/// is centred vertically in the band, so widening the band adds ledger room equally above and
/// below. Widened (166 → ~199 → 220) so 2–3 ledger-line notes fit and ottava brackets pinned to
/// the band edges clear the transposed notes with room to spare.
const BAND_H: f32 = 220.0;
/// Sharps/flats render a bit smaller than other glyphs.
const ACCIDENTAL_SCALE: f32 = 0.8;
/// Flags render smaller so they don't visually dominate at this font size.
const FLAG_SCALE: f32 = 0.63;
const VISIBLE_MEASURES: f32 = 3.0;
/// Playback lead-in in seconds (matches `MidiPlayer`'s 3s lead-in). The minimum scroll speed is
/// derived from this so the band starts empty and notes flow in from the right edge.
const LEAD_IN_SECS: f32 = 3.0;
/// Height of the soft alpha fade at the top/bottom edges of the band (blends into the waterfall).
const EDGE_FADE: f32 = 14.0;
const GRADIENT_STEPS: usize = 10;

// SMuFL: 1em == 4 staff spaces == staff height, so a notehead spans exactly one staff space at
// font size `4 * LINE_SPACING`.
const FONT_SIZE: f32 = 4.0 * LINE_SPACING;

// ---- colors ----------------------------------------------------------------
const COL_BG: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
const COL_STAFF_LINE: [f32; 4] = [0.42, 0.42, 0.42, 1.0];
const COL_BARLINE: [f32; 4] = [0.55, 0.55, 0.55, 1.0];
const COL_PLAYHEAD: [f32; 4] = [0.85, 0.85, 0.85, 0.9];
const COL_CLEF_BLOCK_EDGE: [f32; 4] = [0.35, 0.35, 0.35, 1.0];
const COL_LEDGER: [f32; 4] = [0.7, 0.7, 0.7, 1.0];
const COL_STEM: [f32; 4] = [0.9, 0.9, 0.9, 1.0];
/// Stem/beam colour for a note/group once it has crossed the playhead — matches the noteheads'
/// played colour so stems, beams and heads grey out together.
///
/// Noteheads are glyphs rendered via glyphon in `ColorMode::Accurate`, which treats their colour
/// (COL_GRAY = sRGB 140) correctly. Stems/beams are plain quads whose shader writes colour values
/// straight to the sRGB surface, so wgpu interprets them as *linear* and converts back to sRGB on
/// output. To land on the same perceived grey we must pass the linear equivalent of sRGB 140:
/// srgb_to_linear(140/255) ≈ 0.262. (COL_STEM = 0.9 linear ↔ sRGB ~242, which is why the unplayed
/// white noteheads ~245 already match.)
const COL_STEM_PLAYED: [f32; 4] = [0.262, 0.262, 0.262, 1.0];

const COL_WHITE: Color = Color::rgb(245, 245, 245);
const COL_GRAY: Color = Color::rgb(140, 140, 140);

// ---- glyph placement calibration -------------------------------------------
// Glyphs are placed by their measured bounding-box center; these offsets fine-tune alignment to
// staff coordinates (tune visually).
const GLYPH_DX: f32 = 0.0;
const GLYPH_DY: f32 = 0.0;

/// One deferred draw command, resolved in the commit phase.
enum Cmd {
    Quad(QuadInstance),
    Glyph(GlyphTask),
    /// A short text label (Roboto, e.g. "8va") drawn centred at (x, y).
    Label { text: &'static str, x: f32, y: f32 },
}

/// A single SMuFL char centered at (x, y) with a color and render scale.
struct GlyphTask {
    ch: char,
    x: f32,
    y: f32,
    color: Color,
    scale: f32,
}

/// A deferred text label (default font, not SMuFL) centred at (x, y).
struct LabelTask {
    text: &'static str,
    x: f32,
    y: f32,
}

/// Resolved geometry of one notehead within a voice (single staff): its screen y, diatonic step,
/// notehead glyph, and flag count (0 = quarter-or-longer, 1 = eighth, 2 = sixteenth, ...).
struct VN {
    y: f32,
    step: i32,
    head: char,
    has_stem: bool,
    flags: u8,
    midi: u8,
}

/// One "column" in a voice stream: every note sharing a single start time on one staff (a chord),
/// together with its draw position and play-state color. Consecutive columns are the units that
/// beaming groups together.
#[derive(Clone)]
struct VoiceColumn<'a> {
    t: f32,
    notes: Vec<&'a MidiNote>,
    x: f32,
    color: Color,
}

/// A maximal run of consecutive columns that all need the same octave bracket: notes too high get
/// an 8va/15ma bracket above the staff (drawn shifted down), notes too low get an 8vb/15mb bracket
/// below (drawn shifted up). `shift` is the octave count (1 = 8va/8vb, 2 = 15ma/15mb).
struct OttavaRun {
    start: usize,
    end: usize,
    shift: u8,
    high: bool,
}

pub struct StaffRenderer {
    notes: NoteList,
    measures: Arc<[Duration]>,
    time_signature: (u8, u8),
    key_signature: i8,

    // Voice (声部) assignment per MIDI track. Stems follow the voice, not the absolute pitch:
    // the high voice (right hand) always stems up, the low voice (left hand) always stems down.
    // Each track is mapped to treble/bass by its average pitch, so a hand keeps a consistent
    // staff and stem direction even when it crosses middle C.
    track_is_treble: HashMap<usize, bool>,

    quads: QuadRenderer,
    text: TextRenderer,
    glyphs: glyphs::GlyphCache,
    /// Cached Roboto buffers for the four ottava labels ("8va", "15ma", "8vb", "15mb") with their
    /// measured (width, height), so the bracket line can be broken around the centred label.
    labels: HashMap<&'static str, (glyphon::Buffer, f32, f32)>,

    // layout (set in resize/compute_vertical_layout)
    win_w: f32,
    band_h: f32,
    treble_top: f32,
    treble_bottom: f32,
    bass_top: f32,
    bass_bottom: f32,
}

fn quad(x: f32, y: f32, w: f32, h: f32, color: [f32; 4]) -> QuadInstance {
    QuadInstance {
        position: [x, y],
        size: [w, h],
        color,
        border_radius: [0.0; 4],
    }
}

/// Classify a note by its duration in beats -> (notehead glyph, has stem, flag count).
fn classify(beats: f32) -> (char, bool, u8) {
    use glyphs::cp::*;
    if beats >= 3.0 {
        (NOTEHEAD_WHOLE, false, 0)
    } else if beats >= 1.5 {
        (NOTEHEAD_HALF, true, 0)
    } else if beats >= 0.75 {
        (NOTEHEAD_BLACK, true, 0)
    } else if beats >= 0.375 {
        (NOTEHEAD_BLACK, true, 1)
    } else if beats >= 0.1875 {
        (NOTEHEAD_BLACK, true, 2)
    } else {
        (NOTEHEAD_BLACK, true, 3)
    }
}

/// Diatonic letter (0=C .. 6=B) of a MIDI note (sharps share the natural's letter).
fn note_letter(midi: u8) -> i32 {
    const DEGREE: [i32; 12] = [0, 0, 1, 1, 2, 3, 3, 4, 4, 5, 5, 6];
    DEGREE[(midi % 12) as usize]
}

/// Letters altered by the key signature (sharps: F C G D A E B ; flats: B E A D G C F).
fn key_signature_letters(key: i8) -> Vec<i32> {
    const SHARPS: [i32; 7] = [3, 0, 4, 1, 5, 2, 6]; // F C G D A E B
    const FLATS: [i32; 7] = [6, 2, 5, 1, 4, 0, 3]; // B E A D G C F
    if key > 0 {
        SHARPS[..key as usize].to_vec()
    } else if key < 0 {
        FLATS[..(-key) as usize].to_vec()
    } else {
        Vec::new()
    }
}

/// Nearest diatonic step that equals `letter` (mod 7) to `center`.
fn nearest_step(letter: i32, center: i32) -> i32 {
    let k = ((center - letter) as f32 / 7.0).round() as i32;
    letter + 7 * k
}

/// Ottava bracket label for a run: above the staff (high) → 8va / 15ma, below (low) → 8vb / 15mb.
fn ottava_label(high: bool, shift: u8) -> &'static str {
    match (high, shift) {
        (true, 1) => "8va",
        (true, _) => "15ma",
        (false, 1) => "8vb",
        (false, _) => "15mb",
    }
}

impl StaffRenderer {
    pub fn new(
        notes: NoteList,
        measures: Arc<[Duration]>,
        time_signature: (u8, u8),
        key_signature: i8,
        quads: QuadRenderer,
        text: TextRenderer,
    ) -> Self {
        let mut s = Self {
            notes,
            measures,
            time_signature,
            key_signature,
            track_is_treble: HashMap::new(),
            quads,
            text,
            glyphs: glyphs::GlyphCache::new(),
            labels: Self::build_label_cache(),
            win_w: 0.0,
            band_h: 0.0,
            treble_top: 0.0,
            treble_bottom: 0.0,
            bass_top: 0.0,
            bass_bottom: 0.0,
        };
        s.compute_vertical_layout();
        s.compute_track_voices();
        s
    }

    /// Build the four ottava label buffers once (Roboto at `OTTAVA_LABEL_SIZE`) with their
    /// measured (width, height), so brackets can break their line around the centred text without
    /// re-measuring every frame.
    fn build_label_cache() -> HashMap<&'static str, (glyphon::Buffer, f32, f32)> {
        let mut m = HashMap::new();
        for text in ["8va", "15ma", "8vb", "15mb"] {
            let buf = TextRenderer::gen_buffer(OTTAVA_LABEL_SIZE, text);
            let (w, h) = TextRenderer::measure(&buf);
            m.insert(text, (buf, w, h));
        }
        m
    }

    /// Assign each MIDI track to a voice (treble = high voice / right hand, bass = low voice /
    /// left hand) by comparing the tracks' average pitches. With two or more melody tracks the
    /// highest-average one is treble and the rest are bass, so each hand keeps a consistent
    /// staff and stem direction even when it crosses middle C. With a single track there is no
    /// voice information, so the map is left empty and `draw_chord` falls back to a per-note
    /// pitch split.
    fn compute_track_voices(&mut self) {
        let mut sums: HashMap<usize, (f64, u64)> = HashMap::new();
        for n in self.notes.inner.iter() {
            let e = sums.entry(n.track_id).or_insert((0.0, 0));
            e.0 += n.note as f64;
            e.1 += 1;
        }
        let mut avgs: Vec<(usize, f64)> = sums
            .into_iter()
            .filter(|(_, (_, c))| *c > 0)
            .map(|(id, (s, c))| (id, s / c as f64))
            .collect();
        if avgs.len() < 2 {
            // 0 or 1 melody tracks: no meaningful hand split — leave empty so draw_chord splits
            // each chord by pitch instead.
            return;
        }
        // Highest-average track is the high voice (treble); every other track is bass.
        avgs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        let treble_id = avgs[0].0;
        for (id, _) in &avgs {
            self.track_is_treble.insert(*id, *id == treble_id);
        }
    }

    fn compute_vertical_layout(&mut self) {
        // Net height of the staff system (top offset + treble + gap + bass), without the band
        // padding. Used to centre the system inside the fixed `BAND_H` region.
        let staff_net = TREBLE_TOP_OFFSET + 4.0 * LINE_SPACING + INTER_STAFF_GAP + 4.0 * LINE_SPACING;
        let pad = ((BAND_H - staff_net) * 0.5).max(0.0);
        self.treble_top = STAFF_TOP_Y + pad + TREBLE_TOP_OFFSET;
        self.treble_bottom = self.treble_top + 4.0 * LINE_SPACING;
        self.bass_top = self.treble_bottom + INTER_STAFF_GAP;
        self.bass_bottom = self.bass_top + 4.0 * LINE_SPACING;
        self.band_h = BAND_H;
    }

    pub fn resize(&mut self, win_w: f32) {
        self.win_w = win_w;
    }

    fn y_for_step(&self, treble: bool, step: i32) -> f32 {
        let top_step = pitch::diatonic_step(pitch::top_line_midi(treble));
        let top_y = if treble { self.treble_top } else { self.bass_top };
        // one diatonic step == half a line gap
        top_y - (step - top_step) as f32 * (LINE_SPACING * 0.5)
    }

    /// (measure index, fraction within) for a time in seconds.
    fn measure_frac(&self, time: f32) -> (usize, f32) {
        let m = &self.measures;
        if m.len() < 2 {
            return (0, 0.0);
        }
        let mut i = m.partition_point(|d| d.as_secs_f32() <= time);
        if i > 0 {
            i -= 1;
        }
        if i >= m.len() - 1 {
            i = m.len() - 2;
        }
        let start = m[i].as_secs_f32();
        let end = m[i + 1].as_secs_f32();
        let f = if end > start {
            ((time - start) / (end - start)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        (i, f)
    }

    fn measure_duration_sec(&self, i: usize) -> f32 {
        let m = &self.measures;
        if i + 1 < m.len() {
            (m[i + 1] - m[i]).as_secs_f32().max(0.1)
        } else {
            1.0
        }
    }

    #[profiling::function]
    pub fn update(&mut self, time: f32, physical_size: dpi::PhysicalSize<u32>, scale: f32) {
        // BUILD phase (only reads self) -> command list
        let cmds = self.build(time);

        // COMMIT phase: disjoint mutable fields
        self.quads.clear();
        let mut tasks: Vec<GlyphTask> = Vec::new();
        let mut labels: Vec<LabelTask> = Vec::new();
        for cmd in cmds {
            match cmd {
                Cmd::Quad(q) => self.quads.push(q),
                Cmd::Glyph(g) => tasks.push(g),
                Cmd::Label { text, x, y } => labels.push(LabelTask { text, x, y }),
            }
        }

        // Prime the glyph cache for every char we need this frame, then resolve via a single
        // immutable borrow (so all TextArea buffer references share one lifetime).
        self.glyphs
            .ensure(tasks.iter().map(|t| t.ch), FONT_SIZE);
        let cache = &self.glyphs;
        let label_cache = &self.labels;
        let mut areas: Vec<glyphon::TextArea> = Vec::with_capacity(tasks.len() + labels.len());
        for t in &tasks {
            let (buffer, w, h, ink) = cache.get_ref(t.ch);
            // Flags are anchored by their true ink box: the SMuFL flag's ink left edge is its
            // stem side. The task's (x, y) is the stem-end point the flag must attach to:
            //   up-stem   -> ink top-left    at the stem top    (t.x, t.y)
            //   down-stem -> ink bottom-left at the stem bottom (t.x, t.y)
            // TextArea `scale` scales the whole layout around (left, top), so the ink box is
            // pre-scaled when solving for (left, top). Other glyphs keep em-box centre anchoring.
            let (left, top, scale) = if glyphs::cp::is_flag(t.ch) {
                let s = t.scale * FLAG_SCALE;
                let (left, top) = if glyphs::cp::is_flag_down(t.ch) {
                    (t.x - ink.x * s, t.y - (ink.y + ink.h) * s)
                } else {
                    (t.x - ink.x * s, t.y - ink.y * s)
                };
                (left, top, s)
            } else {
                let s = t.scale;
                (
                    t.x - (w * s) * 0.5 + GLYPH_DX,
                    t.y - (h * s) * 0.5 + GLYPH_DY,
                    s,
                )
            };
            areas.push(glyphon::TextArea {
                buffer,
                left,
                top,
                scale,
                bounds: glyphon::TextBounds::default(),
                default_color: t.color,
                custom_glyphs: &[],
            });
        }
        // Ottava labels (Roboto): centre each cached buffer on its (x, y).
        for lab in &labels {
            if let Some((buffer, w, h)) = label_cache.get(lab.text) {
                areas.push(glyphon::TextArea {
                    buffer,
                    left: lab.x - w * 0.5,
                    top: lab.y - h * 0.5,
                    scale: 1.0,
                    bounds: glyphon::TextBounds::default(),
                    default_color: COL_WHITE,
                    custom_glyphs: &[],
                });
            }
        }
        self.text
            .update_from_iter(physical_size, scale, areas.into_iter());

        // scissor the band (physical px) so geometry/glyphs never bleed outside it
        let scissor = Rect::new(
            Point::new(0u32, (STAFF_TOP_Y * scale) as u32),
            Size::new((self.win_w * scale) as u32, (self.band_h * scale) as u32),
        );
        self.quads.set_scissor_rect(scissor);
        self.text.set_scissor_rect(scissor);
    }

    /// Background of the band: solid black in the middle, fading to transparent over `EDGE_FADE`
    /// at the top and bottom so the band blends into the waterfall instead of a hard rectangle.
    fn push_bg(&self, out: &mut Vec<Cmd>, win_w: f32) {
        let top = STAFF_TOP_Y;
        let h = self.band_h;
        let steps = GRADIENT_STEPS;
        let step_h = EDGE_FADE / steps as f32;

        // top edge: transparent -> opaque
        for s in 0..steps {
            let a = ((s as f32 + 0.5) / steps as f32).clamp(0.0, 1.0);
            let y = top + s as f32 * step_h;
            out.push(Cmd::Quad(quad(0.0, y, win_w, step_h + 0.5, [0.0, 0.0, 0.0, a])));
        }
        // solid middle
        out.push(Cmd::Quad(quad(
            0.0,
            top + EDGE_FADE,
            win_w,
            (h - 2.0 * EDGE_FADE).max(0.0),
            COL_BG,
        )));
        // bottom edge: opaque -> transparent
        for s in 0..steps {
            let a = (1.0 - (s as f32 + 0.5) / steps as f32).clamp(0.0, 1.0);
            let y = top + h - EDGE_FADE + s as f32 * step_h;
            out.push(Cmd::Quad(quad(0.0, y, win_w, step_h + 0.5, [0.0, 0.0, 0.0, a])));
        }
    }

    /// Build all draw commands for this frame using only `&self`.
    fn build(&self, time: f32) -> Vec<Cmd> {
        let win_w = self.win_w;
        let mut out = Vec::new();
        if win_w <= 0.0 {
            return out;
        }

        let playhead_x = win_w * 0.5;
        let (mi, _mf) = self.measure_frac(time);
        let measure_dur = self.measure_duration_sec(mi);

        // Horizontal scroll speed in px/second. We map position by TIME (like the waterfall, not
        // by measure) so the entry behaviour is tempo-independent. `desired` targets ~3 measures
        // across the band; `min_speed` guarantees the 3s lead-in fully clears the right edge, so
        // the staff starts empty and notes flow in from the right (matching the waterfall).
        let desired = (win_w - CLEF_BLOCK_W) / (VISIBLE_MEASURES * measure_dur);
        let min_speed = (win_w - playhead_x + 40.0) / LEAD_IN_SECS;
        let speed = desired.max(min_speed);

        // visible time window
        let lo_t = time - (playhead_x - CLEF_BLOCK_W) / speed;
        let hi_t = time + (win_w - playhead_x) / speed;

        // band background with a soft top/bottom alpha fade so it blends into the waterfall
        self.push_bg(&mut out, win_w);

        // staff lines (treble + bass)
        for treble in [true, false] {
            let top_y = if treble { self.treble_top } else { self.bass_top };
            for i in 0..5 {
                out.push(Cmd::Quad(quad(
                    0.0,
                    top_y + i as f32 * LINE_SPACING,
                    win_w,
                    1.0,
                    COL_STAFF_LINE,
                )));
            }
        }

        // bar lines: map each visible measure's start time to x
        let first_m = self
            .measures
            .partition_point(|d| d.as_secs_f32() < lo_t);
        for i in first_m..self.measures.len() {
            let mt = self.measures[i].as_secs_f32();
            if mt > hi_t {
                break;
            }
            let x = playhead_x + (mt - time) * speed;
            if !(CLEF_BLOCK_W..=win_w).contains(&x) {
                continue;
            }
            // nudge the drawn line left so the downbeat notehead sits in its own measure
            out.push(Cmd::Quad(quad(
                x - BARLINE_NUDGE,
                self.treble_top,
                1.0,
                self.bass_bottom - self.treble_top,
                COL_BARLINE,
            )));
        }

        // clef block edge + play head
        out.push(Cmd::Quad(quad(
            CLEF_BLOCK_W,
            STAFF_TOP_Y,
            1.0,
            self.band_h,
            COL_CLEF_BLOCK_EDGE,
        )));
        out.push(Cmd::Quad(quad(
            playhead_x,
            STAFF_TOP_Y,
            2.0,
            self.band_h,
            COL_PLAYHEAD,
        )));

        // fixed clef/key/time block glyphs
        self.draw_clef_block(&mut out);

        // notes (sorted by start == sorted by x); bound to the visible time window. Drum channel
        // (10) isn't rendered. Each note is routed to a staff (voice); consecutive notes on a staff
        // that share a beat and a duration value are then beamed together (共享符尾).
        let start_idx = self
            .notes
            .inner
            .partition_point(|n| n.start.as_secs_f32() < lo_t);
        let mut visible: Vec<(&MidiNote, f32, Color)> = Vec::new();
        for n in self.notes.inner.iter().skip(start_idx) {
            let t = n.start.as_secs_f32();
            if t > hi_t {
                break;
            }
            if n.channel == 9 {
                continue;
            }
            let x = playhead_x + (t - time) * speed;
            if !(CLEF_BLOCK_W..=win_w).contains(&x) {
                continue;
            }
            let color = if t <= time { COL_GRAY } else { COL_WHITE };
            visible.push((n, x, color));
        }

        // Draw each voice (staff) as its own stream so beaming can connect notes across time.
        for treble in [true, false] {
            let mut columns: Vec<VoiceColumn> = Vec::new();
            for &(n, x, color) in &visible {
                if self.note_is_treble(n) != treble {
                    continue;
                }
                let t = n.start.as_secs_f32();
                // consecutive same-start notes form one column (a chord on this staff)
                if let Some(last) = columns.last_mut() {
                    if last.t == t {
                        last.notes.push(n);
                        continue;
                    }
                }
                columns.push(VoiceColumn {
                    t,
                    notes: vec![n],
                    x,
                    color,
                });
            }
            if !columns.is_empty() {
                self.draw_voice_stream(&columns, treble, time, playhead_x, speed, &mut out);
            }
        }

        out
    }

    fn draw_clef_block(&self, out: &mut Vec<Cmd>) {
        // clefs centered on each staff's middle line
        out.push(Cmd::Glyph(GlyphTask {
            ch: glyphs::cp::G_CLEF,
            x: 14.0,
            y: (self.treble_top + self.treble_bottom) * 0.5,
            color: COL_WHITE,
            scale: 1.0,
        }));
        out.push(Cmd::Glyph(GlyphTask {
            ch: glyphs::cp::F_CLEF,
            x: 14.0,
            y: (self.bass_top + self.bass_bottom) * 0.5,
            color: COL_WHITE,
            scale: 1.0,
        }));

        // key signature accidentals (canonical-ish staff positions)
        let mut x = 26.0;
        for treble in [true, false] {
            let center = pitch::diatonic_step(if treble { 71 } else { 50 }); // B4 / D3
            let ch = if self.key_signature >= 0 {
                glyphs::cp::SHARP
            } else {
                glyphs::cp::FLAT
            };
            for &letter in key_signature_letters(self.key_signature).iter() {
                let step = nearest_step(letter, center);
                let y = self.y_for_step(treble, step);
                out.push(Cmd::Glyph(GlyphTask {
                    ch,
                    x,
                    y,
                    color: COL_WHITE,
                    scale: ACCIDENTAL_SCALE,
                }));
                x += LINE_SPACING * 1.1;
            }
        }

        // time signature (numerator over denominator) just left of the play head
        let (num, den) = self.time_signature;
        let ts_x = CLEF_BLOCK_W - 12.0;
        let base = glyphs::cp::TIME_SIG_0 as u32;
        for treble in [true, false] {
            let mid = if treble {
                (self.treble_top + self.treble_bottom) * 0.5
            } else {
                (self.bass_top + self.bass_bottom) * 0.5
            };
            for (digit, off) in [(num, -LINE_SPACING), (den, LINE_SPACING)] {
                if let Some(c) = char::from_u32(base + digit as u32) {
                    out.push(Cmd::Glyph(GlyphTask {
                        ch: c,
                        x: ts_x,
                        y: mid + off,
                        color: COL_WHITE,
                        scale: 1.0,
                    }));
                }
            }
        }
    }

    /// Which staff (voice) a note belongs to. With two or more melody tracks the right-hand track
    /// is always the high voice (treble, stem up) and the left-hand track the low voice (bass, stem
    /// down), even where a hand crosses middle C. With a single track there is no hand information,
    /// so the note splits by pitch at middle C as a fallback.
    fn note_is_treble(&self, n: &MidiNote) -> bool {
        if !self.track_is_treble.is_empty() {
            self.track_is_treble
                .get(&n.track_id)
                .copied()
                .unwrap_or(false)
        } else {
            pitch::is_treble(n.note)
        }
    }

    /// Decide whether a beam group starts at column `i`, and if so return `(last_index, flags)`
    /// — the last column included (inclusive) and the beam count (1 = eighth, 2 = sixteenth,
    /// 3 = thirty-second). Returns `None` for a lone / non-beamable column.
    ///
    /// The rhythmic value is taken from the **inter-onset gap** between consecutive columns, not
    /// from each note's stored MIDI duration: real files often give the first note of a run a long
    /// (held / overlapping) duration that would otherwise classify it as a quarter and split it off
    /// the beam — exactly the "single note + group of 3" artefact. The run extends while
    /// consecutive gaps keep the same value, stays inside one quarter beat (beams never cross a
    /// beat), and is capped at the count of that value fitting one beat (eighths → 2, sixteenths →
    /// 4, thirty-seconds → 8), so a beam never holds more notes than a beat's worth.
    fn beam_run_end(
        &self,
        columns: &[VoiceColumn],
        i: usize,
        quarter: f32,
        ms: f32,
    ) -> Option<(usize, u8)> {
        let n = columns.len();
        if i + 1 >= n {
            return None;
        }
        // Compute the end of the beat containing columns[i].t. A tiny epsilon is added BEFORE
        // flooring so that a note whose (t-ms)/quarter is mathematically an integer (a note right
        // on a beat boundary) isn't pushed to the previous beat by float error — that mis-placed
        // boundary then hard-cut the beam and split a beat of 4 sixteenths into 1 + 3.
        let beat_idx = ((columns[i].t - ms) / quarter + quarter * 0.01).floor();
        let beat_end = ms + (beat_idx + 1.0) * quarter;
        let eps = quarter * 0.02; // absorb float error at the exact beat boundary
        if columns[i + 1].t >= beat_end - eps {
            return None;
        }
        let f = classify((columns[i + 1].t - columns[i].t) / quarter).2;
        if f < 1 {
            return None;
        }
        let cap = match f {
            1 => 2,
            2 => 4,
            _ => 8,
        };
        let mut j = i + 1;
        while j + 1 < n && (j + 1 - i) < cap && columns[j + 1].t < beat_end - eps {
            if classify((columns[j + 1].t - columns[j].t) / quarter).2 != f {
                break;
            }
            j += 1;
        }
        Some((j, f))
    }

    /// Resolve the per-notehead geometry for one column on `staff`. `shift` (diatonic steps) is
    /// applied to the staff step only — for ottava, so the notehead renders transposed while its
    /// midi (and thus accidental) stays true.
    fn voice_vns(
        &self,
        notes: &[&MidiNote],
        treble: bool,
        quarter: f32,
        shift: i32,
    ) -> Vec<VN> {
        notes
            .iter()
            .map(|n| {
                let step = pitch::diatonic_step(n.note) + shift;
                let y = self.y_for_step(treble, step);
                let beats = n.duration.as_secs_f32() / quarter.max(0.001);
                let (head, has_stem, flags) = classify(beats);
                VN { y, step, head, has_stem, flags, midi: n.note }
            })
            .collect()
    }

    /// Ledger lines + accidentals + noteheads for `vns` at horizontal `x`, all scaled by
    /// `head_scale` (1.0 for normal notes, smaller for grace notes). Shared by the lone column
    /// path ([`Self::draw_voice`]), the beamed-group path ([`Self::draw_beamed_group`]), and the
    /// grace-note path ([`Self::draw_grace`]).
    fn draw_note_decos(
        &self,
        vns: &[VN],
        treble: bool,
        x: f32,
        head_scale: f32,
        color: Color,
        out: &mut Vec<Cmd>,
    ) {
        let top_step = pitch::diatonic_step(pitch::top_line_midi(treble));
        let bounds = pitch::StaffBounds {
            top: top_step,
            bottom: top_step - 8,
        };
        // ledger lines (per note)
        for vn in vns {
            for ls in bounds.ledger_steps(vn.step) {
                let ly = self.y_for_step(treble, ls);
                out.push(Cmd::Quad(quad(
                    x - LINE_SPACING * 0.8 * head_scale,
                    ly,
                    LINE_SPACING * 1.6 * head_scale,
                    1.0,
                    COL_LEDGER,
                )));
            }
        }
        // accidentals (per note)
        for vn in vns {
            if pitch::is_black_key(vn.midi)
                && !key_signature_letters(self.key_signature).contains(&note_letter(vn.midi))
            {
                let ch = if self.key_signature >= 0 {
                    glyphs::cp::SHARP
                } else {
                    glyphs::cp::FLAT
                };
                out.push(Cmd::Glyph(GlyphTask {
                    ch,
                    x: x - LINE_SPACING * 1.1 * head_scale,
                    y: vn.y,
                    color,
                    scale: ACCIDENTAL_SCALE * head_scale,
                }));
            }
        }
        // noteheads
        for vn in vns {
            out.push(Cmd::Glyph(GlyphTask {
                ch: vn.head,
                x,
                y: vn.y,
                color,
                scale: head_scale,
            }));
        }
    }

    /// Draw one column (notes on one staff, same start time): decorations plus a single shared
    /// stem + flag. The stem is measured from the outermost head — highest pitch for treble (stem
    /// up), lowest pitch for bass (stem down) — and extends `stem_len` past it, so the flag sits at
    /// the true stem end, not near the heads. This is the lone-column path; beamed columns go
    /// through [`Self::draw_beamed_group`].
    fn draw_voice(
        &self,
        voice: &[&MidiNote],
        x: f32,
        treble: bool,
        color: Color,
        shift: i32,
        out: &mut Vec<Cmd>,
    ) {
        let stem_down = !treble;
        let stem_len = 3.0 * LINE_SPACING;
        let stem_w = 1.2;
        let nh_half = LINE_SPACING * 0.5;

        // Note value (relative to a quarter) is shared: every note in the chord starts together,
        // so the measure / quarter duration is the same for all of them.
        let (mi, _) = self.measure_frac(voice[0].start.as_secs_f32());
        let quarter = self.measure_duration_sec(mi) * 0.25;

        let vns = self.voice_vns(voice, treble, quarter, shift);
        self.draw_note_decos(&vns, treble, x, 1.0, color, out);

        // shared stem + flag, from the outermost head
        let any_stem = vns.iter().any(|v| v.has_stem);
        if any_stem {
            let flags = vns.iter().map(|v| v.flags).max().unwrap_or(0);
            // Outermost head on the flag side: stem-up -> highest pitch (min screen y);
            // stem-down -> lowest pitch (max screen y). The other extreme is where the stem
            // begins.
            let y_hi = vns.iter().map(|v| v.y).fold(f32::INFINITY, f32::min);
            let y_lo = vns.iter().map(|v| v.y).fold(f32::NEG_INFINITY, f32::max);
            let stem_x = if stem_down {
                x - nh_half
            } else {
                x + nh_half - stem_w
            };
            let (quad_top, quad_h, flag_y) = if stem_down {
                // from the highest head down to `stem_len` past the lowest head
                let bottom = y_lo + stem_len;
                (y_hi, bottom - y_hi, bottom)
            } else {
                // from the lowest head up to `stem_len` past the highest head
                let top = y_hi - stem_len;
                (top, y_lo - top, top)
            };
            out.push(Cmd::Quad(quad(
                stem_x,
                quad_top,
                stem_w,
                quad_h,
                if color.r() < 200 { COL_STEM_PLAYED } else { COL_STEM },
            )));
            if flags > 0 {
                let ch = glyphs::cp::flag(flags, stem_down);
                // The flag attaches at the stem's left edge (stem_x) and the stem end (flag_y);
                // the ink box is anchored to this point in the placement loop in `update`.
                out.push(Cmd::Glyph(GlyphTask {
                    ch,
                    x: stem_x,
                    y: flag_y,
                    color,
                    scale: 1.0,
                }));
            }
        }
    }

    /// Find maximal runs of consecutive columns whose notes need an octave bracket — i.e. they'd
    /// land beyond the 3 ledger lines we have room for (more than `OTTAVA_LEDGER_MAX` diatonic
    /// steps past a staff edge). Each run is uniformly high (8va/15ma) or low (8vb/15mb); a column
    /// with both an out-of-range high and low note is treated as high. The shift is the smallest
    /// octave count (capped at 2) that brings the run's extreme note back inside the range.
    fn ottava_runs(&self, columns: &[VoiceColumn], treble: bool) -> Vec<OttavaRun> {
        let top = pitch::diatonic_step(pitch::top_line_midi(treble));
        let bottom = top - 8;
        let kind_of = |col: &VoiceColumn| -> Option<bool> {
            let max_step = col
                .notes
                .iter()
                .map(|n| pitch::diatonic_step(n.note))
                .max()
                .unwrap_or(top);
            let min_step = col
                .notes
                .iter()
                .map(|n| pitch::diatonic_step(n.note))
                .min()
                .unwrap_or(top);
            if max_step - top > OTTAVA_LEDGER_MAX {
                Some(true)
            } else if bottom - min_step > OTTAVA_LEDGER_MAX {
                Some(false)
            } else {
                None
            }
        };
        let mut runs = Vec::new();
        let n = columns.len();
        let mut i = 0;
        while i < n {
            let Some(is_high) = kind_of(&columns[i]) else {
                i += 1;
                continue;
            };
            let mut j = i;
            let mut run_max = i32::MIN;
            let mut run_min = i32::MAX;
            while j < n && kind_of(&columns[j]) == Some(is_high) {
                for nn in &columns[j].notes {
                    let s = pitch::diatonic_step(nn.note);
                    run_max = run_max.max(s);
                    run_min = run_min.min(s);
                }
                j += 1;
            }
            let shift = if is_high {
                let excess = run_max - (top + OTTAVA_LEDGER_MAX);
                (((excess + 6) / 7).max(1) as u8).min(2)
            } else {
                let excess = (bottom - OTTAVA_LEDGER_MAX) - run_min;
                (((excess + 6) / 7).max(1) as u8).min(2)
            };
            runs.push(OttavaRun { start: i, end: j - 1, shift, high: is_high });
            i = j;
        }
        runs
    }

    /// Draw one ottava bracket: a horizontal line (broken around the centred label) with a short
    /// hook at each end pointing toward the staff — a "lying [". Pinned to the band's top edge for
    /// 8va/15ma (high notes shifted down) or the bottom edge for 8vb/15mb (low notes shifted up),
    /// so it sits clear of the transposed notes on both staves. Line + hooks are quads; the label
    /// is deferred via [`Cmd::Label`].
    fn draw_ottava_bracket(
        &self,
        cols: &[VoiceColumn],
        run: &OttavaRun,
        out: &mut Vec<Cmd>,
    ) {
        let start_x = cols[run.start].x - LINE_SPACING * 0.9;
        let end_x = cols[run.end].x + LINE_SPACING * 0.9;
        // Pin the bracket to the band's top edge (8va, high notes shift down) or bottom edge (8vb,
        // low notes shift up) instead of sitting just outside a single staff, maximising clearance
        // from the transposed notes. `label_half` keeps the glyph inside the scissor rect; the label
        // may overlap the soft EDGE_FADE band but stays readable.
        let label_half = OTTAVA_LABEL_SIZE * 0.5;
        let (bracket_y, hooks_down) = if run.high {
            (STAFF_TOP_Y + label_half, true)
        } else {
            (STAFF_TOP_Y + BAND_H - label_half, false)
        };

        let label = ottava_label(run.high, run.shift);
        let label_w = self
            .labels
            .get(label)
            .map(|(_, w, _)| *w)
            .unwrap_or(OTTAVA_LABEL_SIZE * 2.0);
        let mid_x = (start_x + end_x) * 0.5;
        let half = label_w * 0.5 + LINE_SPACING * 0.5;
        let thick = 1.2;
        let hook_len = LINE_SPACING * 0.9;

        // horizontal line, broken around the centred label
        let left_w = (mid_x - half) - start_x;
        if left_w > 0.0 {
            out.push(Cmd::Quad(quad(
                start_x,
                bracket_y - thick * 0.5,
                left_w,
                thick,
                COL_STEM,
            )));
        }
        let right_x = mid_x + half;
        let right_w = end_x - right_x;
        if right_w > 0.0 {
            out.push(Cmd::Quad(quad(
                right_x,
                bracket_y - thick * 0.5,
                right_w,
                thick,
                COL_STEM,
            )));
        }
        // hooks at both ends, pointing toward the staff
        let (hy, hh) = if hooks_down {
            (bracket_y, hook_len)
        } else {
            (bracket_y - hook_len, hook_len)
        };
        out.push(Cmd::Quad(quad(start_x, hy, thick, hh, COL_STEM)));
        out.push(Cmd::Quad(quad(end_x - thick, hy, thick, hh, COL_STEM)));
        // centred label
        out.push(Cmd::Label {
            text: label,
            x: mid_x,
            y: bracket_y,
        });
    }

    /// Walk one voice's columns (sorted by time). First, columns whose notes fall outside the
    /// staff's 3-ledger-line range are grouped into ottava runs ([`Self::ottava_runs`]) and drawn
    /// transposed under a bracket ([`Self::draw_ottava_bracket`]); the per-column shift is applied
    /// to every draw path below. Then a column that is a grace note (very short note crushed just
    /// before a longer main note) is drawn small via [`Self::draw_grace`] and skipped past;
    /// otherwise each column either starts a beamed group ([`Self::beam_run_end`]) or — if lone /
    /// not beamable — is drawn through [`Self::draw_voice`] with an individual flag.
    /// Build an extended column set for ottava run detection: the visible `columns` sit in the
    /// middle, padded with extra (off-screen) columns on **both** sides — `OTTAVA_LOOKAHEAD_MEASURES`
    /// worth to the left and to the right. This lets an ottava run's `start`/`end` resolve to its
    /// true first/last note even when those notes have already scrolled off the left edge or haven't
    /// entered yet, so the bracket is drawn at its full final length and neither grows (right side)
    /// nor shrinks (left side) as notes slide in and out of view.
    ///
    /// Returns the extended vector plus `vis_offset` — the index in it where the visible `columns`
    /// block begins (the visible block occupies `[vis_offset, vis_offset + columns.len())`).
    ///
    /// The extension reuses the same note→staff routing (`note_is_treble`) and x-mapping
    /// (`playhead_x + (t - time) * speed`) as `build()`, so extended columns line up exactly with
    /// the visible ones. Color is irrelevant to ottava detection, so extended columns use
    /// `COL_WHITE` as a placeholder.
    fn extend_columns<'a>(
        &'a self,
        columns: &[VoiceColumn<'a>],
        treble: bool,
        time: f32,
        playhead_x: f32,
        speed: f32,
    ) -> (Vec<VoiceColumn<'a>>, usize) {
        let n = columns.len();
        if speed <= 0.0 || n == 0 {
            return (columns.to_vec(), 0);
        }

        let first_t = columns[0].t;
        let last_t = columns[n - 1].t;
        let (mi, _) = self.measure_frac(last_t);
        let measure_dur = self.measure_duration_sec(mi);
        let span = OTTAVA_LOOKAHEAD_MEASURES as f32 * measure_dur;
        let ext_lo_t = first_t - span;
        let ext_hi_t = last_t + span;

        // Collect the extended columns from self.notes.inner over [ext_lo_t, ext_hi_t], excluding
        // the visible window (first_t..=last_t) which is already in `columns`. Group same-start
        // notes into one column (a chord), matching build()'s logic.
        let mut left: Vec<VoiceColumn<'a>> = Vec::new();
        let mut right: Vec<VoiceColumn<'a>> = Vec::new();
        let start_idx = self
            .notes
            .inner
            .partition_point(|nn| nn.start.as_secs_f32() < ext_lo_t);
        for nn in self.notes.inner.iter().skip(start_idx) {
            let t = nn.start.as_secs_f32();
            if t > ext_hi_t {
                break;
            }
            if nn.channel == 9 {
                continue;
            }
            if self.note_is_treble(nn) != treble {
                continue;
            }
            // Inside the visible window — already covered by `columns`.
            if t >= first_t && t <= last_t {
                continue;
            }
            let x = playhead_x + (t - time) * speed;
            let bucket = if t < first_t { &mut left } else { &mut right };
            if let Some(last) = bucket.last_mut() {
                if last.t == t {
                    last.notes.push(nn);
                    continue;
                }
            }
            bucket.push(VoiceColumn {
                t,
                notes: vec![nn],
                x,
                color: COL_WHITE,
            });
        }

        // Assemble: [left (ascending)] + [visible columns] + [right (ascending)].
        // `left` was collected ascending already (forward scan), but we built it by push, and the
        // scan is ascending so left is ascending. Good. Record where the visible block starts.
        let vis_offset = left.len();
        let mut ext = left;
        ext.extend_from_slice(columns);
        ext.extend(right);
        (ext, vis_offset)
    }

    fn draw_voice_stream(
        &self,
        columns: &[VoiceColumn],
        treble: bool,
        time: f32,
        playhead_x: f32,
        speed: f32,
        out: &mut Vec<Cmd>,
    ) {
        let n = columns.len();
        if n == 0 {
            return;
        }

        // Ottava (8va/8vb): per-column draw-shift (diatonic steps) so out-of-range notes render
        // back into the staff — high notes shift down (8va, bracket above), low notes shift up
        // (8vb, bracket below).
        //
        // To keep the bracket at a stable full length, the run scan uses an *extended* column set
        // that pads the visible window on BOTH sides by a few measures: a run's `start`/`end` then
        // resolve to its true first/last note even when those have scrolled off the left or haven't
        // entered from the right yet. The visible `columns` sit in `ext_columns` at
        // `[vis_offset, vis_offset + n)`; run indices (into ext) map back to visible indices by
        // subtracting `vis_offset` and clamping to the visible block.
        let (ext_columns, vis_offset) =
            self.extend_columns(columns, treble, time, playhead_x, speed);
        let runs = self.ottava_runs(&ext_columns, treble);
        // col_shift is indexed by *visible* `columns`. For each ext-run, translate its [start..=end]
        // span to visible indices and apply the shift where they overlap the visible block. Clamp to
        // the visible block *before* subtracting `vis_offset`: a run that sits entirely in the left
        // padding has `end + 1 < vis_offset`, and subtracting first would underflow usize and walk
        // `k` far past `s.len()` (crash seen with Op_299_p24-25.mid).
        let vis_end = vis_offset + n; // exclusive
        let col_shift: Vec<i32> = {
            let mut s = vec![0i32; n];
            for r in &runs {
                let v = (if r.high { -1 } else { 1 }) * 7 * r.shift as i32;
                let lo = r.start.clamp(vis_offset, vis_end) - vis_offset;
                let hi = (r.end.saturating_add(1).clamp(vis_offset, vis_end)) - vis_offset;
                for k in lo..hi {
                    s[k] = v;
                }
            }
            s
        };
        for r in &runs {
            // Only draw a bracket if its span overlaps the visible block; runs entirely off-screen
            // (left or right) produce nothing to see.
            if r.start < vis_end && r.end >= vis_offset {
                self.draw_ottava_bracket(&ext_columns, r, out);
            }
        }

        let mut i = 0;
        while i < n {
            let (mi, _) = self.measure_frac(columns[i].t);
            let quarter = self.measure_duration_sec(mi) * 0.25;
            let ms = self.measures.get(mi).map(|d| d.as_secs_f32()).unwrap_or(0.0);

            // Grace note (倚音): a two-note "short crush + main" group. The grace is a note whose
            // stored duration is ≤ a 32nd, crushed immediately before a longer main note of any
            // value. Classified by stored duration (not inter-onset gap) so arpeggios (short gap,
            // long held duration) aren't misread as grace.
            let q = quarter.max(0.001);
            let is_grace = i + 1 < n
                && columns[i]
                    .notes
                    .iter()
                    .all(|n| classify(n.duration.as_secs_f32() / q).2 >= 3)
                && columns[i + 1]
                    .notes
                    .iter()
                    .any(|n| classify(n.duration.as_secs_f32() / q).2 < 3)
                && columns[i + 1].t - columns[i].t <= quarter * 0.5;
            if is_grace {
                self.draw_grace(&columns[i], columns[i + 1].x, treble, col_shift[i], out);
                i += 1;
                continue;
            }

            match self.beam_run_end(columns, i, quarter, ms) {
                Some((j, f)) => {
                    self.draw_beamed_group(&columns[i..=j], treble, f, col_shift[i], out);
                    i = j + 1;
                }
                None => {
                    self.draw_voice(
                        &columns[i].notes,
                        columns[i].x,
                        treble,
                        columns[i].color,
                        col_shift[i],
                        out,
                    );
                    i += 1;
                }
            }
        }
    }

    /// Draw a grace note (倚音) column: a smaller notehead (with a short stem) placed just left of
    /// the following main notehead (`main_x`), instead of at the grace's literal (overlapping) time
    /// position. No beam/flag — the small size and pre-main placement read as an ornament.
    fn draw_grace(
        &self,
        col: &VoiceColumn,
        main_x: f32,
        treble: bool,
        shift: i32,
        out: &mut Vec<Cmd>,
    ) {
        let stem_down = !treble;
        let nh_half = LINE_SPACING * 0.5 * GRACE_SCALE;
        let stem_w = 1.0;
        let gx = main_x - GRACE_OFFSET;

        let vns: Vec<VN> = col
            .notes
            .iter()
            .map(|n| {
                let step = pitch::diatonic_step(n.note) + shift;
                let y = self.y_for_step(treble, step);
                VN {
                    y,
                    step,
                    head: glyphs::cp::NOTEHEAD_BLACK,
                    has_stem: true,
                    flags: 0,
                    midi: n.note,
                }
            })
            .collect();
        self.draw_note_decos(&vns, treble, gx, GRACE_SCALE, col.color, out);

        // short stem from the outermost head (highest for stem-up, lowest for stem-down)
        let y_hi = vns.iter().map(|v| v.y).fold(f32::INFINITY, f32::min);
        let y_lo = vns.iter().map(|v| v.y).fold(f32::NEG_INFINITY, f32::max);
        let stem_x = if stem_down {
            gx - nh_half
        } else {
            gx + nh_half - stem_w
        };
        let (top, h) = if stem_down {
            let bottom = y_lo + GRACE_STEM_LEN;
            (y_hi, bottom - y_hi)
        } else {
            let top = y_hi - GRACE_STEM_LEN;
            (top, y_lo - top)
        };
        out.push(Cmd::Quad(quad(
            stem_x,
            top,
            stem_w,
            h,
            if col.color.r() < 200 { COL_STEM_PLAYED } else { COL_STEM },
        )));
    }

    /// Draw a beamed group: `flags` parallel horizontal beams (8th = 1, 16th = 2, 32nd = 3)
    /// connecting one stem per column. Beams are flat because the renderer's quads are
    /// axis-aligned (no rotation), so a slanted beam can't be drawn as a single shape. Each stem
    /// runs from its column's outermost head to the beam line; the primary beam sits `stem_len`
    /// past the outermost head of the whole group, and secondary beams step toward the heads.
    fn draw_beamed_group(
        &self,
        cols: &[VoiceColumn],
        treble: bool,
        flags: u8,
        shift: i32,
        out: &mut Vec<Cmd>,
    ) {
        let stem_down = !treble;
        let stem_len = 3.0 * LINE_SPACING;
        let stem_w = 1.2;
        let nh_half = LINE_SPACING * 0.5;
        let beam_thick = LINE_SPACING * 0.3;
        let beam_gap = LINE_SPACING * 0.5; // centre-to-centre between parallel beams
        // A beamed group reads as one unit, so its stems/beam grey out as a whole — following the
        // LAST note: only once the final notehead has crossed the playhead do the stems and beam
        // switch to the played colour, matching the noteheads.
        let played = cols
            .last()
            .is_some_and(|c| c.color.r() < 200); // COL_GRAY (140) vs COL_WHITE (245)
        let stem_col = if played { COL_STEM_PLAYED } else { COL_STEM };

        let (mi, _) = self.measure_frac(cols[0].t);
        let quarter = self.measure_duration_sec(mi) * 0.25;

        // per-column notehead geometry + decorations
        let col_vns: Vec<Vec<VN>> = cols
            .iter()
            .map(|c| {
                let mut vns = self.voice_vns(&c.notes, treble, quarter, shift);
                // a beamed note is 8th-or-shorter by definition → always a filled notehead,
                // regardless of the (often long, held) stored MIDI duration that `voice_vns`
                // classified from.
                for v in vns.iter_mut() {
                    v.head = glyphs::cp::NOTEHEAD_BLACK;
                }
                vns
            })
            .collect();
        for (c, vns) in cols.iter().zip(col_vns.iter()) {
            self.draw_note_decos(vns, treble, c.x, 1.0, c.color, out);
        }

        // outermost head per column on the beam side:
        //   stem-up   -> highest pitch (min screen y)
        //   stem-down -> lowest pitch  (max screen y)
        let outer_y: Vec<f32> = col_vns
            .iter()
            .map(|vns| {
                if stem_down {
                    vns.iter().map(|v| v.y).fold(f32::NEG_INFINITY, f32::max)
                } else {
                    vns.iter().map(|v| v.y).fold(f32::INFINITY, f32::min)
                }
            })
            .collect();

        // primary beam line: flat, `stem_len` past the outermost head of the whole group
        let group_outer = if stem_down {
            outer_y.iter().copied().fold(f32::NEG_INFINITY, f32::max)
        } else {
            outer_y.iter().copied().fold(f32::INFINITY, f32::min)
        };
        let beam_y = if stem_down {
            group_outer + stem_len
        } else {
            group_outer - stem_len
        };

        // one stem per column, head -> beam line
        for (k, c) in cols.iter().enumerate() {
            let stem_x = if stem_down {
                c.x - nh_half
            } else {
                c.x + nh_half - stem_w
            };
            let (top, h) = if stem_down {
                (outer_y[k], beam_y - outer_y[k])
            } else {
                (beam_y, outer_y[k] - beam_y)
            };
            out.push(Cmd::Quad(quad(stem_x, top, stem_w, h, stem_col)));
        }

        // beams: cap the stems exactly — first stem's left edge to last stem's right edge
        let first_x = cols.first().unwrap().x;
        let last_x = cols.last().unwrap().x;
        let (bx, bw) = if stem_down {
            (first_x - nh_half, (last_x - first_x) + stem_w)
        } else {
            (first_x + nh_half - stem_w, (last_x - first_x) + stem_w)
        };
        for b in 0..flags {
            // primary beam outermost; secondary beams step toward the heads
            let y = if stem_down {
                beam_y - b as f32 * beam_gap
            } else {
                beam_y + b as f32 * beam_gap
            };
            out.push(Cmd::Quad(quad(bx, y, bw, beam_thick, stem_col)));
        }
    }

    pub fn prepare(&mut self) {
        self.quads.prepare();
    }

    pub fn render<'rpass>(&'rpass mut self, rpass: &mut wgpu_jumpstart::RenderPass<'rpass>) {
        self.quads.render(rpass);
        self.text.render(rpass);
    }
}
