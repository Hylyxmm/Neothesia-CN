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
const LINE_SPACING: f32 = 11.2; // distance between two adjacent staff lines
const TREBLE_TOP_OFFSET: f32 = 14.0;
// 2 line-spacings so middle C (C4) is a single shared ledger line between the two staves.
const INTER_STAFF_GAP: f32 = 2.0 * LINE_SPACING;
const CLEF_BLOCK_W: f32 = 80.0;
/// Fixed vertical extent of the staff band (top at `STAFF_TOP_Y`). Decoupled from
/// `LINE_SPACING` so shrinking the line spacing keeps the region (and its scissor) the same
/// size — the smaller staff is centred within it instead of being clipped top/bottom.
/// Value = the old `14 + 10*LINE_SPACING + 12` evaluated at LINE_SPACING = 14.
const BAND_H: f32 = 166.0;
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
}

/// A single SMuFL char centered at (x, y) with a color and render scale.
struct GlyphTask {
    ch: char,
    x: f32,
    y: f32,
    color: Color,
    scale: f32,
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
        for cmd in cmds {
            match cmd {
                Cmd::Quad(q) => self.quads.push(q),
                Cmd::Glyph(g) => tasks.push(g),
            }
        }

        // Prime the glyph cache for every char we need this frame, then resolve via a single
        // immutable borrow (so all TextArea buffer references share one lifetime).
        self.glyphs
            .ensure(tasks.iter().map(|t| t.ch), FONT_SIZE);
        let cache = &self.glyphs;
        let mut areas: Vec<glyphon::TextArea> = Vec::with_capacity(tasks.len());
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
            out.push(Cmd::Quad(quad(
                x,
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

        // notes (sorted by start == sorted by x); bound to the visible time window. Notes that
        // share a start time are a chord and are drawn with one shared stem per staff.
        let start_idx = self
            .notes
            .inner
            .partition_point(|n| n.start.as_secs_f32() < lo_t);
        let mut notes = self.notes.inner.iter().skip(start_idx).peekable();
        while let Some(note) = notes.next() {
            let t = note.start.as_secs_f32();
            if t > hi_t {
                break;
            }
            // gather every note at this same start time (the rest of the chord); they are
            // consecutive because the slice is sorted by start.
            let mut group: Vec<&MidiNote> = vec![note];
            while let Some(next) = notes.peek() {
                if next.start.as_secs_f32() == t {
                    group.push(notes.next().unwrap());
                } else {
                    break;
                }
            }
            // drum channel (10) isn't rendered on the staff
            let group: Vec<&MidiNote> = group.into_iter().filter(|n| n.channel != 9).collect();
            if group.is_empty() {
                continue;
            }
            let x = playhead_x + (t - time) * speed;
            if !(CLEF_BLOCK_W..=win_w).contains(&x) {
                continue;
            }
            let color = if t <= time { COL_GRAY } else { COL_WHITE };
            self.draw_chord(&group, x, color, &mut out);
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

    /// Draw a chord: every note sharing one start time. The chord may span both staves (two
    /// hands), so each staff is drawn as a separate voice with its own single shared stem.
    ///
    /// Voice assignment follows the MIDI track (hand), not the absolute pitch: when the file has
    /// two melody tracks the right-hand track is always the high voice (stem up) and the
    /// left-hand track the low voice (stem down), even where a hand crosses middle C. With a
    /// single track there is no hand information, so notes split by pitch as a fallback.
    fn draw_chord(&self, group: &[&MidiNote], x: f32, color: Color, out: &mut Vec<Cmd>) {
        let voiced = !self.track_is_treble.is_empty();
        for treble in [true, false] {
            let voice: Vec<&MidiNote> = group
                .iter()
                .copied()
                .filter(|n| {
                    if voiced {
                        // Stem follows the hand/voice (track), not the pitch.
                        self.track_is_treble.get(&n.track_id).copied().unwrap_or(false) == treble
                    } else {
                        // No track voice info: fall back to a per-note pitch split at middle C.
                        pitch::is_treble(n.note) == treble
                    }
                })
                .collect();
            if voice.is_empty() {
                continue;
            }
            self.draw_voice(&voice, x, treble, color, out);
        }
    }

    /// Draw one voice (notes on one staff, same start time): noteheads / accidentals / ledger
    /// lines per note, plus a single shared stem + flag. The stem is measured from the outermost
    /// head — highest pitch for treble (stem up), lowest pitch for bass (stem down) — and extends
    /// `stem_len` past it, so the flag sits at the true stem end, not near the heads.
    fn draw_voice(
        &self,
        voice: &[&MidiNote],
        x: f32,
        treble: bool,
        color: Color,
        out: &mut Vec<Cmd>,
    ) {
        let stem_down = !treble;
        let stem_len = 3.0 * LINE_SPACING;
        let stem_w = 1.2;
        let nh_half = LINE_SPACING * 0.5;

        // Note value (relative to a quarter) is shared: every note in the chord starts together,
        // so the measure / quarter duration is the same for all of them.
        let (ni, _) = self.measure_frac(voice[0].start.as_secs_f32());
        let quarter = self.measure_duration_sec(ni) * 0.25;

        struct VN {
            y: f32,
            step: i32,
            head: char,
            has_stem: bool,
            flags: u8,
            midi: u8,
        }
        let vns: Vec<VN> = voice
            .iter()
            .map(|n| {
                let step = pitch::diatonic_step(n.note);
                let y = self.y_for_step(treble, step);
                let beats = n.duration.as_secs_f32() / quarter.max(0.001);
                let (head, has_stem, flags) = classify(beats);
                VN { y, step, head, has_stem, flags, midi: n.note }
            })
            .collect();

        // ledger lines (per note)
        let top_step = pitch::diatonic_step(pitch::top_line_midi(treble));
        let bounds = pitch::StaffBounds {
            top: top_step,
            bottom: top_step - 8,
        };
        for vn in &vns {
            for ls in bounds.ledger_steps(vn.step) {
                let ly = self.y_for_step(treble, ls);
                out.push(Cmd::Quad(quad(
                    x - LINE_SPACING * 0.8,
                    ly,
                    LINE_SPACING * 1.6,
                    1.0,
                    COL_LEDGER,
                )));
            }
        }

        // accidentals (per note)
        for vn in &vns {
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
                    x: x - LINE_SPACING * 1.1,
                    y: vn.y,
                    color,
                    scale: ACCIDENTAL_SCALE,
                }));
            }
        }

        // noteheads
        for vn in &vns {
            out.push(Cmd::Glyph(GlyphTask {
                ch: vn.head,
                x,
                y: vn.y,
                color,
                scale: 1.0,
            }));
        }

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
            out.push(Cmd::Quad(quad(stem_x, quad_top, stem_w, quad_h, COL_STEM)));
            if flags > 0 {
                let ch = glyphs::cp::flag(flags, stem_down);
                // The flag attaches at the stem's left edge (stem_x) and the stem end (flag_y);
                // the ink box is anchored to this point in the placement loop above.
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

    pub fn prepare(&mut self) {
        self.quads.prepare();
    }

    pub fn render<'rpass>(&'rpass mut self, rpass: &mut wgpu_jumpstart::RenderPass<'rpass>) {
        self.quads.render(rpass);
        self.text.render(rpass);
    }
}
