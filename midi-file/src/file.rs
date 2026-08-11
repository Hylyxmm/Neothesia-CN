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
                time = tempo_track.pulses_to_duration(id * u_per_quarter_note as u64 * 4);
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
