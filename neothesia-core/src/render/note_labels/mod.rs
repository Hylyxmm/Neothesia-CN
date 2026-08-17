use crate::utils::Point;

use super::{KeyboardRenderer, TextRenderer, waterfall::NoteList};

#[derive(Default)]
struct LabelsCache {
    labels: Option<[glyphon::Buffer; 12]>,
    neutral_width: f32,
}

impl LabelsCache {
    #[profiling::function]
    fn get(&mut self, keyboard: &KeyboardRenderer) -> &[glyphon::Buffer; 12] {
        let font_system = crate::font_system::font_system();
        let font_system = &mut font_system.borrow_mut();

        let sharp_width = keyboard.layout().sizing.sharp_width;
        let neutral_width = keyboard.layout().sizing.neutral_width;

        if self.labels.is_none() || self.neutral_width != neutral_width {
            let label_width = sharp_width;

            let labels = [
                ("C", neutral_width),
                ("C#", sharp_width),
                ("D", neutral_width),
                ("D#", sharp_width),
                ("E", neutral_width),
                ("F", neutral_width),
                ("F#", sharp_width),
                ("G", neutral_width),
                ("G#", sharp_width),
                ("A", neutral_width),
                ("A#", sharp_width),
                ("B", neutral_width),
            ]
            .map(|(label, note_width)| {
                let mut buffer = glyphon::Buffer::new(
                    font_system,
                    glyphon::Metrics::new(label_width, label_width),
                );
                buffer.set_size(Some(note_width), None);
                buffer.set_wrap(glyphon::Wrap::None);
                buffer.set_text(
                    label,
                    &glyphon::Attrs::new().family(glyphon::Family::SansSerif),
                    glyphon::Shaping::Basic,
                    Some(glyphon::cosmic_text::Align::Center),
                );
                buffer.shape_until_scroll(font_system, false);
                buffer
            });

            self.labels = Some(labels);
            self.neutral_width = neutral_width;
        }

        self.labels.as_ref().unwrap()
    }
}

pub struct NoteLabels {
    pos: Point<f32>,
    notes: NoteList,
    labels_cache: LabelsCache,
    text_renderer: TextRenderer,
}

impl NoteLabels {
    pub fn new(pos: Point<f32>, notes: &NoteList, text_renderer: TextRenderer) -> Self {
        Self {
            pos,
            notes: notes.clone(),
            labels_cache: LabelsCache::default(),
            text_renderer,
        }
    }

    pub fn set_pos(&mut self, pos: Point<f32>) {
        self.pos = pos;
    }

    #[profiling::function]
    pub fn update(
        &mut self,
        physical_size: dpi::PhysicalSize<u32>,
        scale: f32,
        keyboard: &KeyboardRenderer,
        animation_speed: f32,
        time: f32,
    ) {
        let layout = keyboard.layout();
        let range_start = layout.range.start() as usize;
        let label_width = layout.sizing.sharp_width;

        let labels = self.labels_cache.get(keyboard);
        let animation_speed = animation_speed / scale;

        let iter = self
            .notes
            .inner
            .iter()
            .filter(|note| layout.range.contains(note.note) && note.channel != 9)
            .map(|note| {
                let buffer = &labels[(note.note % 12) as usize];

                let x = layout.keys[note.note as usize - range_start].x();
                // The label sits one label-height above the note's bottom trajectory (inside the
                // block). For short blocks (block height < label height) that would hover ABOVE
                // the block's top and get clipped by the screen edge while sliding in — so clamp
                // the label to just inside the block's top instead. The 0.1s min matches the
                // waterfall's own duration clamp so the block top is computed identically.
                let note_bottom =
                    self.pos.y - (note.start.as_secs_f32() - time) * animation_speed;
                let block_top = note_bottom
                    - note.duration.as_secs_f32().max(0.1) * animation_speed;
                let y = (note_bottom - label_width).max(block_top + 2.0);

                (buffer, x, y)
            })
            // Stop iteration once we reach top of the screen
            .take_while(|(_buffer, _x, y)| *y > 0.0)
            // Skip notes already past the waterfall's bottom anchor (the judging line). This is
            // `self.pos.y` (the anchor the scene positions us at) rather than the keyboard's own
            // pos, so it also follows the screen bottom when the keyboard is hidden.
            .skip_while(|(_buffer, _x, y)| *y > self.pos.y)
            .map(|(buffer, left, top)| glyphon::TextArea {
                buffer,
                left,
                top,
                scale: 1.0,
                bounds: glyphon::TextBounds {
                    left: 0,
                    top: 0,
                    right: i32::MAX,
                    bottom: i32::MAX,
                },
                default_color: glyphon::Color::rgb(255, 255, 255),
                custom_glyphs: &[],
            });

        self.text_renderer
            .update_from_iter(physical_size, scale, iter);
    }

    pub fn render<'rpass>(&'rpass mut self, render_pass: &mut wgpu_jumpstart::RenderPass<'rpass>) {
        self.text_renderer.render(render_pass);
    }
}
