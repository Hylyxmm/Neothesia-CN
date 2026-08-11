//! Pitch -> staff position mapping for the grand staff.

/// Diatonic "staff step" of a MIDI note. Each white key increases the step by 1; a sharp shares
/// its natural's step (e.g. C# == C). MIDI 0 == C-1, so `octave = midi/12 - 1`.
///
/// Going up one step == one staff position (line -> space -> line ...). Two steps == one
/// "line gap" (line to the next line).
pub fn diatonic_step(midi: u8) -> i32 {
    // degree per pitch class (C..B): naturals and their sharps share a degree
    const DEGREE: [i32; 12] = [0, 0, 1, 1, 2, 3, 3, 4, 4, 5, 5, 6];
    let octave = (midi / 12) as i32 - 1;
    octave * 7 + DEGREE[(midi % 12) as usize]
}

/// Which staff a note belongs to. Split at middle C (MIDI 60): C4 and above -> treble, below ->
/// bass.
pub fn is_treble(midi: u8) -> bool {
    midi >= 60
}

/// MIDI note of the *top line* of each staff. Treble top line = F5 (77), bass top line = A3 (57).
pub fn top_line_midi(treble: bool) -> u8 {
    if treble { 77 } else { 57 }
}

/// Step of the outermost staff line that a note beyond the staff would cross first, when going
/// outward. Returns (line_step, going_down). For treble: notes below the staff cross line 30 (E4)
/// first; notes above cross 40 (G5) first. We pass the staff's bottom/top line steps.
pub struct StaffBounds {
    /// step of the top line
    pub top: i32,
    /// step of the bottom line
    pub bottom: i32,
}

impl StaffBounds {
    /// Ledger-line staff positions (every other step, continuing the line pattern) that sit
    /// between the staff edge and `note_step` (inclusive of the line just outside the note).
    /// Returns their diatonic steps (caller converts to y).
    pub fn ledger_steps(&self, note_step: i32) -> Vec<i32> {
        let mut out = Vec::new();
        if note_step < self.bottom {
            // ledger lines continue below: bottom-2, bottom-4, ... while >= note_step (same parity as bottom)
            let mut s = self.bottom - 2;
            while s >= note_step {
                out.push(s);
                s -= 2;
            }
        } else if note_step > self.top {
            let mut s = self.top + 2;
            while s <= note_step {
                out.push(s);
                s += 2;
            }
        }
        out
    }
}

/// Pitch class accidental for a note, as the offset in semitones from its natural: 0 (white key),
/// or +1 (a black key). Used together with the key signature to decide whether to draw a sharp or
/// flat.
pub fn is_black_key(midi: u8) -> bool {
    matches!(midi % 12, 1 | 3 | 6 | 8 | 10)
}
