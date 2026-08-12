use crate::{MidiTrack, program_track::ProgramTrack, tempo_track::TempoTrack};
use midly::{Format, MetaMessage, Smf, Timing, TrackEventKind};
use std::{fs, path::Path, sync::Arc};

#[derive(Debug, Clone)]
pub struct MidiFile {
    pub name: String,
    pub format: Format,
    pub tracks: Arc<[MidiTrack]>,
    pub program_track: ProgramTrack,
    pub tempo_track: TempoTrack,
    pub measures: Arc<[std::time::Duration]>,
    /// (numerator, real denominator) parsed from the first `TimeSignature` meta event.
    pub time_signature: (u8, u8),
    /// Number of sharps in the key signature (negative = flats), from the first `KeySignature`
    /// meta event. Used by the staff renderer to draw the key signature and accidentals.
    pub key_signature: i8,
}

impl MidiFile {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let name = path
            .as_ref()
            .file_name()
            .ok_or(String::from("File not found"))?
            .to_string_lossy()
            .to_string();

        let data = match fs::read(path) {
            Ok(buff) => buff,
            Err(_) => return Err(String::from("Could Not Open File")),
        };

        let smf = match Smf::parse(&data) {
            Ok(smf) => smf,
            Err(_) => return Err(String::from("Midi Parsing Error (midly lib)")),
        };

        Self::from_parsed_smf(name, &smf)
    }

    pub fn from_smf(name: impl Into<String>, smf: &Smf<'_>) -> Result<Self, String> {
        Self::from_parsed_smf(name.into(), smf)
    }

    fn from_parsed_smf(name: String, smf: &Smf<'_>) -> Result<Self, String> {
        let u_per_quarter_note: u16 = match smf.header.timing {
            Timing::Metrical(t) => t.as_int(),
            Timing::Timecode(_fps, _u) => {
                return Err(String::from("Midi With Timecode Timing, Not Supported!"));
            }
        };

        if smf.tracks.is_empty() {
            return Err(String::from("Midi File Has No Tracks"));
        }

        let tempo_track = TempoTrack::build(&smf.tracks, u_per_quarter_note);

        // Parse the first TimeSignature / KeySignature meta events across all tracks.
        // TimeSignature stores the denominator as a power-of-two exponent (raw byte),
        // so the real denominator is `1 << raw`. KeySignature stores sharps as i8
        // (negative means flats).
        let mut time_signature = (4u8, 4u8);
        let mut key_signature = 0i8;
        for track in &smf.tracks {
            for ev in track.iter() {
                if let TrackEventKind::Meta(meta) = &ev.kind {
                    match meta {
                        MetaMessage::TimeSignature(num, den_raw, _, _) => {
                            time_signature = (*num, 1u8 << (*den_raw).min(7) as u32);
                        }
                        MetaMessage::KeySignature(sharps, _minor) => {
                            key_signature = *sharps;
                        }
                        _ => {}
                    }
                }
            }
        }

        let mut track_color_id = 0;
        let tracks: Vec<MidiTrack> = smf
            .tracks
            .iter()
            .enumerate()
            .map(|(id, events)| {
                let track = MidiTrack::new(id, track_color_id, &tempo_track, events);

                if !track.notes.is_empty() {
                    track_color_id += 1;
                }

                track
            })
            .collect();

        // One measure spans `numerator` beats of `1/denominator` each. In PPQ terms a quarter
        // note is `u_per_quarter_note` ticks, so a beat is `u_per_quarter_note * 4 / denominator`
        // ticks and a measure is `u_per_quarter_note * 4 * numerator / denominator` ticks. This
        // replaces the old hard-coded 4/4 assumption (which was just the `* 4 / 4` special case).
        let (ts_num, ts_den) = time_signature;
        let ticks_per_beat = (u_per_quarter_note as u64 * 4) / ts_den as u64;
        let ticks_per_measure = ticks_per_beat * ts_num as u64;

        let measures = {
            let last_note_end = tracks
                .iter()
                .fold(std::time::Duration::ZERO, |last, track| {
                    if let Some(note) = track.notes.last() {
                        last.max(note.start + note.duration)
                    } else {
                        last
                    }
                });

            let mut masures = Vec::new();
            let mut time = std::time::Duration::ZERO;
            let mut id = 0;
            while time <= last_note_end {
                time = tempo_track.pulses_to_duration(id * ticks_per_measure);
                masures.push(time);
                id += 1;
            }

            masures
        };

        let program_track = ProgramTrack::new(&tracks);

        Ok(Self {
            name,
            format: smf.header.format,
            tracks: tracks.into(),
            program_track,
            tempo_track,
            measures: measures.into(),
            time_signature,
            key_signature,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use midly::{Format, Header, MetaMessage, MidiMessage, Smf, Timing, Track, TrackEvent};

    /// Build a one-track SMf at the given PPQ and time signature, with `measures` measures each
    /// holding `beats_per_measure` quarter-note beats of a single repeating note. Returns the
    /// parsed `MidiFile` plus the expected ticks-per-measure.
    fn build(ppq: u16, num: u8, den: u8, measures: usize, beats_per_measure: u32) -> (MidiFile, u64) {
        let mut track = Track::new();
        // TimeSignature meta: denominator stored as power-of-two exponent.
        let den_raw = (den as f64).log2().round() as u8;
        track.push(TrackEvent {
            delta: 0.into(),
            kind: TrackEventKind::Meta(MetaMessage::TimeSignature(num, den_raw, 24, 8)),
        });
        let quarter = ppq as u32;
        for _ in 0..measures {
            for _ in 0..beats_per_measure {
                track.push(TrackEvent {
                    delta: 0.into(),
                    kind: TrackEventKind::Midi {
                        channel: 0.into(),
                        message: MidiMessage::NoteOn {
                            key: 60.into(),
                            vel: 80.into(),
                        },
                    },
                });
                track.push(TrackEvent {
                    delta: quarter.into(),
                    kind: TrackEventKind::Midi {
                        channel: 0.into(),
                        message: MidiMessage::NoteOff {
                            key: 60.into(),
                            vel: 0.into(),
                        },
                    },
                });
            }
        }
        track.push(TrackEvent {
            delta: 0.into(),
            kind: TrackEventKind::Meta(MetaMessage::EndOfTrack),
        });

        let smf = Smf {
            header: Header::new(Format::SingleTrack, Timing::Metrical(ppq.into())),
            tracks: vec![track],
        };
        let midi = MidiFile::from_parsed_smf("test".into(), &smf).unwrap();
        let ticks_per_measure = (ppq as u64 * 4 / den as u64) * num as u64;
        (midi, ticks_per_measure)
    }

    /// Measures should be evenly spaced by exactly one measure's worth of ticks (converted to
    /// seconds at the default 120 BPM tempo), regardless of time signature.
    fn assert_measure_spacing(midi: &MidiFile, ticks_per_measure: u64, ppq: u16) {
        assert!(midi.measures.len() >= 2, "need >=2 measures");
        // At 120 BPM a quarter note = 0.5s, so ticks_per_measure -> seconds:
        let expected_sec = ticks_per_measure as f64 * 0.5 / ppq as f64;
        for w in midi.measures.windows(2) {
            let gap = w[1].as_secs_f64() - w[0].as_secs_f64();
            assert!(
                (gap - expected_sec).abs() < 1e-3,
                "measure gap {gap}s != expected {expected_sec}s (ts spacing wrong)"
            );
        }
    }

    #[test]
    fn measure_width_4_4() {
        // 4/4: 4 quarter beats per measure -> 4 * ppq ticks. The legacy hard-coded behaviour.
        let (midi, tpm) = build(480, 4, 4, 4, 4);
        assert_eq!(midi.time_signature, (4, 4));
        assert_eq!(tpm, 1920);
        assert_measure_spacing(&midi, tpm, 480);
    }

    #[test]
    fn measure_width_3_4() {
        // 3/4: 3 quarter beats per measure -> 3 * ppq = 1440 ticks (not 1920).
        let (midi, tpm) = build(480, 3, 4, 4, 3);
        assert_eq!(midi.time_signature, (3, 4));
        assert_eq!(tpm, 1440);
        assert_measure_spacing(&midi, tpm, 480);
    }

    #[test]
    fn measure_width_6_8() {
        // 6/8: 6 eighth beats. A measure = ppq*4*6/8 = ppq*3 = 1440 ticks.
        // beats_per_measure here counts quarter-note beats (6 eighths = 3 quarters).
        let (midi, tpm) = build(480, 6, 8, 4, 3);
        assert_eq!(midi.time_signature, (6, 8));
        assert_eq!(tpm, 1440);
        assert_measure_spacing(&midi, tpm, 480);
    }

    #[test]
    fn measure_width_2_2() {
        // 2/2 (alla breve): 2 half-note beats -> ppq*4*2/2 = ppq*4 = 1920 ticks, but only 2
        // quarter-note beats of content per measure.
        let (midi, tpm) = build(480, 2, 2, 4, 2);
        assert_eq!(midi.time_signature, (2, 2));
        assert_eq!(tpm, 1920);
        assert_measure_spacing(&midi, tpm, 480);
    }
}
