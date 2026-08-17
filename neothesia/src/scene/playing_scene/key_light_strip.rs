//! A thin indicator light strip along the screen bottom, shown when the virtual keyboard is
//! hidden (external-piano setup). It mirrors the real keyboard's key positions in two rows —
//! light grey segments over white keys, dark grey over black keys — so the player can align the
//! screen with the physical keys below. When a key is pressed (user input or file playback), the
//! matching segment lights up in the colour of the pressing note block, with a per-pixel soft
//! glow rising into the waterfall and a few gently rising sparkle particles.

use std::time::Duration;

use neothesia_core::render::{KeyboardKeyState, LightRenderer};
use piano_layout::KeyboardLayout;
use wgpu_jumpstart::{Gpu, TransformUniform, Uniform};

/// Per-row strip height (logical px).
const STRIP_H: f32 = 3.0;
/// Total strip height: the white-key row sits at the very bottom, the black-key row stacked on
/// top of it (mirroring a real keyboard's layout). The scene keeps the waterfall's judging line
/// this far above the screen bottom so the strip never overlaps falling note blocks.
pub const STRIP_TOTAL_H: f32 = 2.0 * STRIP_H;
/// Idle segment colours: light grey over white keys, dark grey over black keys.
const WHITE_BASE: [f32; 4] = [0.55, 0.55, 0.55, 1.0];
const BLACK_BASE: [f32; 4] = [0.22, 0.22, 0.22, 1.0];
/// Fallback lit colour (the UI's accent purple) until a note colour is known; the strip normally
/// lights up in the colour of the note block that pressed the key.
const LIT_FALLBACK_RGB: [f32; 3] = [160.0 / 255.0, 81.0 / 255.0, 1.0];
/// Brightness smoothing time constant (s) — soft attack/release of the light.
const FADE_TAU: f32 = 0.09;
/// Height (logical px) of the glow rising from a lit segment into the waterfall area, fading
/// per-pixel from the segment's top edge to zero.
const GLOW_H: f32 = 90.0;
/// Gap (logical px) between adjacent segments, so neighbouring white keys read as distinct.
const SEGMENT_GAP: f32 = 1.0;
/// Peak alpha of the glow at the segment's top edge; horizontal feather fraction of the width.
const GLOW_PEAK_ALPHA: f32 = 0.75;
const GLOW_EDGE: f32 = 0.35;

/// One rising sparkle: spawns at a freshly pressed segment, drifts upward and fades out.
struct Particle {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    life: f32,
    max_life: f32,
    radius: f32,
    color: [f32; 3],
}

pub struct KeyLightStrip {
    renderer: LightRenderer,
    /// Per-key brightness, 0 (idle) .. 1 (fully lit), smoothed over time.
    brightness: Vec<f32>,
    /// Per-key lit colour — the colour of the note block that pressed the key, normalised to
    /// full brightness so the strip matches the waterfall's hue but reads as a light.
    lit_color: Vec<[f32; 3]>,
    /// Last frame's pressed state, to detect press edges for particle spawning.
    prev_pressed: Vec<bool>,
    particles: Vec<Particle>,
    /// Monotonic frame counter, used as a cheap entropy source for particle variation.
    tick: u32,
}

/// Deterministic pseudo-random in [0, 1) — a multiply-xorshift hash; good enough for sparkles.
fn hash01(n: u32) -> f32 {
    let x = n.wrapping_mul(2654435761).rotate_left(13);
    ((x >> 8) & 0xffff) as f32 / 65536.0
}

impl KeyLightStrip {
    pub fn new(gpu: &Gpu, transform: &Uniform<TransformUniform>) -> Self {
        Self {
            renderer: LightRenderer::new(gpu, transform),
            brightness: Vec::new(),
            lit_color: Vec::new(),
            prev_pressed: Vec::new(),
            particles: Vec::new(),
            tick: 0,
        }
    }

    pub fn update(&mut self, delta: Duration, key_states: &[KeyboardKeyState], layout: &KeyboardLayout) {
        self.tick = self.tick.wrapping_add(1);
        let n = key_states.len();
        self.brightness.resize(n, 0.0);
        self.lit_color.resize(n, LIT_FALLBACK_RGB);
        self.prev_pressed.resize(n, false);

        let dt = delta.as_secs_f32();

        for i in 0..n {
            // Light up on user input AND on file playback (auto-play), taking the note's own
            // colour so the strip matches the waterfall block that pressed the key. The colour is
            // saturation-boosted (pushed away from grey) and normalised — the same hue as the
            // note block, but deep and vivid rather than pale.
            let press_color = key_states[i]
                .pressed_by_user()
                .or_else(|| key_states[i].pressed_by_file());
            let pressed = press_color.is_some();
            if let Some(c) = press_color {
                const SAT_BOOST: f32 = 1.8;
                let gray = (c.r + c.g + c.b) / 3.0;
                let sat = |v: f32| (gray + (v - gray) * SAT_BOOST).clamp(0.0, 1.0);
                let (r, g, b) = (sat(c.r), sat(c.g), sat(c.b));
                let m = r.max(g).max(b).max(0.001);
                self.lit_color[i] = [r / m, g / m, b / m];
            }
            let target = if pressed { 1.0 } else { 0.0 };
            // Exponential approach towards the target — soft fade in/out.
            let k = (dt / FADE_TAU).min(1.0);
            self.brightness[i] += (target - self.brightness[i]) * k;

            // Spawn a couple of sparkles on the press edge.
            if pressed && !self.prev_pressed[i] {
                let key = &layout.keys[i];
                // Sparkles rise from the top of the key's own row: black keys sit one row higher.
                let row_top = if matches!(key.note_id(), 1 | 3 | 6 | 8 | 10) {
                    STRIP_TOTAL_H
                } else {
                    STRIP_H
                };
                for s in 0..2 {
                    let r1 = hash01(self.tick.wrapping_add(i as u32 * 131).wrapping_add(s * 37));
                    let r2 = hash01(self.tick.wrapping_mul(31).wrapping_add(i as u32 * 7).wrapping_add(s));
                    let r3 = hash01(self.tick.wrapping_add(i as u32).wrapping_add(s * 977));
                    self.particles.push(Particle {
                        x: key.x() + r1 * key.width(),
                        // y is stored relative to the screen bottom (upwards = negative).
                        y: -(row_top + r2 * 4.0),
                        vx: (r3 - 0.5) * 14.0,
                        vy: 18.0 + r2 * 26.0,
                        life: 0.0,
                        max_life: 0.6 + r3 * 0.6,
                        radius: 1.4 + r1 * 1.4,
                        color: self.lit_color[i],
                    });
                }
            }
            self.prev_pressed[i] = pressed;
        }

        // Advance particles (y is relative to the screen bottom; upward = negative).
        for p in &mut self.particles {
            p.life += dt;
            p.x += p.vx * dt;
            p.y -= p.vy * dt;
        }
        self.particles.retain(|p| p.life < p.max_life);
    }

    /// Rebuild this frame's light instances and upload them. `screen_h` is the logical window
    /// height; the strip sits at its very bottom in two rows mirroring a real keyboard.
    pub fn prepare(&mut self, screen_h: f32, layout: &KeyboardLayout) {
        self.renderer.clear();
        let white_top = screen_h - STRIP_H;
        let black_top = screen_h - STRIP_TOTAL_H;

        for (i, key) in layout.keys.iter().enumerate() {
            let is_black = matches!(key.note_id(), 1 | 3 | 6 | 8 | 10);
            let base = if is_black { BLACK_BASE } else { WHITE_BASE };
            let b = self.brightness.get(i).copied().unwrap_or(0.0);
            let lc = self.lit_color.get(i).copied().unwrap_or(LIT_FALLBACK_RGB);
            let row_top = if is_black { black_top } else { white_top };

            // Idle segment (plain rectangle with anti-aliased edges), inset by SEGMENT_GAP so
            // adjacent white keys read as distinct keys.
            self.renderer.push(neothesia_core::render::LightInstance {
                position: [key.x() + SEGMENT_GAP * 0.5, row_top],
                size: [(key.width() - SEGMENT_GAP).max(1.0), STRIP_H],
                color: base,
                params: [0.0, 0.0, 0.0, 0.0],
            });

            // Lit overlay: the note's own colour at full saturation, alpha = brightness. The
            // colour is NOT mixed with the grey base — it layers on top, so it stays vivid.
            if b > 0.01 {
                self.renderer.push(neothesia_core::render::LightInstance {
                    position: [key.x() + SEGMENT_GAP * 0.5, row_top],
                    size: [(key.width() - SEGMENT_GAP).max(1.0), STRIP_H],
                    color: [lc[0], lc[1], lc[2], b],
                    params: [0.0, 0.0, 0.0, 0.0],
                });

                // Glow rising from the segment into the waterfall: one quad with a per-pixel
                // vertical fade (brightest at the bottom, zero at GLOW_H) and feathered sides.
                self.renderer.push(neothesia_core::render::LightInstance {
                    position: [key.x(), row_top - GLOW_H],
                    size: [key.width(), GLOW_H],
                    color: [lc[0], lc[1], lc[2], GLOW_PEAK_ALPHA * b],
                    params: [GLOW_H, 0.0, GLOW_EDGE, 0.0],
                });
            }
        }

        // Sparkles: soft radial dots rising off the strip. Their y is relative to the screen
        // bottom (negative = upward), set at spawn to the key's own row top.
        for p in &self.particles {
            let fade = 1.0 - p.life / p.max_life;
            self.renderer.push(neothesia_core::render::LightInstance {
                position: [p.x - p.radius, screen_h + p.y - p.radius],
                size: [p.radius * 2.0, p.radius * 2.0],
                color: [p.color[0], p.color[1], p.color[2], 0.7 * fade * fade],
                params: [0.0, 1.0, 0.0, 0.0],
            });
        }

        self.renderer.prepare();
    }

    pub fn render<'a>(&'a self, render_pass: &mut wgpu_jumpstart::RenderPass<'a>) {
        self.renderer.render(render_pass);
    }
}
