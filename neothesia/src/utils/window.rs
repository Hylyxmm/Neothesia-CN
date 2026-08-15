use winit::{
    dpi::{LogicalPosition, PhysicalPosition},
    event::{ElementState, KeyEvent, MouseButton},
    keyboard::{Key, ModifiersState},
};

use winit::{
    dpi::{LogicalSize, PhysicalSize},
    event::WindowEvent,
};

/// A monitor as presented in the settings UI: stable `name` (config key) plus a
/// human-readable `label` (shown in the picker button and combo list).
pub struct MonitorEntry {
    pub name: String,
    pub label: String,
}

/// A resolution as presented in the settings UI, deduped by size with the highest
/// refresh rate kept.
pub struct ResolutionEntry {
    pub size: (u32, u32),
    pub refresh_rate_millihertz: u32,
}

impl ResolutionEntry {
    pub fn label(&self) -> String {
        if self.refresh_rate_millihertz > 0 {
            format!(
                "{} × {}（{} Hz）",
                self.size.0,
                self.size.1,
                self.refresh_rate_millihertz / 1000
            )
        } else {
            format!("{} × {}", self.size.0, self.size.1)
        }
    }
}

fn selected_monitor(
    window: &winit::window::Window,
    monitor_name: Option<&str>,
) -> Option<winit::monitor::MonitorHandle> {
    let monitors: Vec<_> = window.available_monitors().collect();
    monitor_name
        .and_then(|name| monitors.iter().find(|m| m.name().as_deref() == Some(name)))
        .cloned()
        .or_else(|| window.current_monitor())
        .or_else(|| window.primary_monitor())
        .or_else(|| monitors.into_iter().next())
}

/// List all monitors for the picker UI.
pub fn list_monitors(window: &winit::window::Window) -> Vec<MonitorEntry> {
    window
        .available_monitors()
        .enumerate()
        .map(|(i, m)| {
            let name = m.name().unwrap_or_else(|| format!("monitor-{i}"));
            let size = m.size();
            let label = format!("{}. {}（{}×{}）", i + 1, name, size.width, size.height);
            MonitorEntry { name, label }
        })
        .collect()
}

/// List the resolutions the given monitor supports, deduped by size (highest refresh
/// rate kept), sorted from largest to smallest.
pub fn list_resolutions(
    window: &winit::window::Window,
    monitor_name: Option<&str>,
) -> Vec<ResolutionEntry> {
    let Some(monitor) = selected_monitor(window, monitor_name) else {
        return Vec::new();
    };

    let mut best: Vec<ResolutionEntry> = Vec::new();
    for mode in monitor.video_modes() {
        let size = (mode.size().width, mode.size().height);
        match best.iter_mut().find(|e| e.size == size) {
            Some(e) => {
                if mode.refresh_rate_millihertz() > e.refresh_rate_millihertz {
                    e.refresh_rate_millihertz = mode.refresh_rate_millihertz();
                }
            }
            None => best.push(ResolutionEntry {
                size,
                refresh_rate_millihertz: mode.refresh_rate_millihertz(),
            }),
        }
    }

    best.sort_by(|a, b| (b.size.0 * b.size.1, b.refresh_rate_millihertz).cmp(&(
        a.size.0 * a.size.1,
        a.refresh_rate_millihertz,
    )));
    best
}

/// Pick the video mode matching the preferred resolution (highest refresh rate), or the
/// monitor's largest mode when no preference is stored.
fn pick_video_mode(
    monitor: &winit::monitor::MonitorHandle,
    resolution: Option<(u32, u32)>,
) -> Option<winit::monitor::VideoModeHandle> {
    let modes: Vec<_> = monitor.video_modes().collect();
    match resolution {
        Some((w, h)) => modes
            .into_iter()
            .filter(|m| m.size().width == w && m.size().height == h)
            .max_by_key(|m| m.refresh_rate_millihertz()),
        None => modes
            .into_iter()
            .max_by_key(|m| (m.size().width * m.size().height, m.refresh_rate_millihertz())),
    }
}

/// Apply the persisted window settings: exclusive fullscreen (selected monitor +
/// resolution) or windowed at the stored size. Safe to call repeatedly.
pub fn apply_window_settings(
    window: &winit::window::Window,
    fullscreen: bool,
    monitor_name: Option<&str>,
    resolution: Option<(u32, u32)>,
) {
    if !fullscreen {
        window.set_fullscreen(None);
        if let Some((w, h)) = resolution {
            let _ = window.request_inner_size(PhysicalSize::new(w, h));
        }
        return;
    }

    let Some(monitor) = selected_monitor(window, monitor_name) else {
        window.set_fullscreen(Some(winit::window::Fullscreen::Borderless(None)));
        return;
    };

    match pick_video_mode(&monitor, resolution) {
        Some(mode) => {
            window.set_fullscreen(Some(winit::window::Fullscreen::Exclusive(mode)));
        }
        None => {
            // Stored resolution not offered by this monitor; fall back to borderless.
            window.set_fullscreen(Some(winit::window::Fullscreen::Borderless(Some(monitor))));
        }
    }
}

pub struct WindowState {
    pub physical_size: PhysicalSize<u32>,
    pub logical_size: LogicalSize<f32>,

    pub scale_factor: f64,

    pub cursor_physical_position: PhysicalPosition<f64>,
    pub cursor_logical_position: LogicalPosition<f32>,

    pub focused: bool,

    pub modifiers_state: ModifiersState,
    pub left_mouse_btn: bool,
    pub right_mouse_btn: bool,
}

impl WindowState {
    pub fn new(window: &winit::window::Window) -> Self {
        let scale_factor = window.scale_factor();

        let (physical_size, logical_size) = {
            let physical_size = window.inner_size();
            let logical_size = physical_size.to_logical::<f32>(scale_factor);
            (physical_size, logical_size)
        };

        let cursor_physical_position = PhysicalPosition::new(0.0, 0.0);
        let cursor_logical_position = LogicalPosition::new(0.0, 0.0);

        Self {
            physical_size,
            logical_size,

            scale_factor,

            cursor_physical_position,
            cursor_logical_position,

            focused: false,

            modifiers_state: ModifiersState::default(),
            left_mouse_btn: false,
            right_mouse_btn: false,
        }
    }

    pub fn window_event(&mut self, event: &WindowEvent) {
        match event {
            // Windows sets size to 0 on minimise
            WindowEvent::Resized(ps) if ps.width > 0 && ps.height > 0 => {
                self.physical_size.width = ps.width;
                self.physical_size.height = ps.height;
                self.logical_size = ps.to_logical(self.scale_factor);
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.logical_size = self.physical_size.to_logical(self.scale_factor);
                self.scale_factor = *scale_factor;
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor_physical_position = *position;
                self.cursor_logical_position = position.to_logical(self.scale_factor);
            }
            WindowEvent::Focused(f) => {
                self.focused = *f;
            }
            WindowEvent::ModifiersChanged(state) => {
                self.modifiers_state = state.state();
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                self.left_mouse_btn = *state == ElementState::Pressed;
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Right,
                ..
            } => {
                self.right_mouse_btn = *state == ElementState::Pressed;
            }
            _ => {}
        }
    }
}

#[allow(unused)]
pub trait WinitEvent {
    fn scale_factor_changed(&self) -> bool;
    fn window_resized(&self) -> bool;
    fn cursor_moved(&self) -> bool;
    fn redraw_requested(&self) -> bool;

    fn mouse_pressed(&self, btn: MouseButton) -> bool;
    fn mouse_released(&self, btn: MouseButton) -> bool;

    fn left_mouse_pressed(&self) -> bool {
        self.mouse_pressed(MouseButton::Left)
    }

    fn left_mouse_released(&self) -> bool {
        self.mouse_released(MouseButton::Left)
    }

    fn right_mouse_pressed(&self) -> bool {
        self.mouse_pressed(MouseButton::Right)
    }

    fn right_mouse_released(&self) -> bool {
        self.mouse_released(MouseButton::Right)
    }

    fn back_mouse_pressed(&self) -> bool {
        self.mouse_pressed(MouseButton::Back)
    }

    fn back_mouse_released(&self) -> bool {
        self.mouse_released(MouseButton::Back)
    }

    fn key_pressed(&self, key: Key<&str>) -> bool;
    fn key_released(&self, key: Key<&str>) -> bool;

    fn character_released(&self) -> Option<&str>;
}

impl WinitEvent for WindowEvent {
    fn scale_factor_changed(&self) -> bool {
        matches!(self, Self::ScaleFactorChanged { .. })
    }

    fn window_resized(&self) -> bool {
        matches!(self, Self::Resized { .. })
    }

    fn cursor_moved(&self) -> bool {
        matches!(self, Self::CursorMoved { .. })
    }

    fn redraw_requested(&self) -> bool {
        matches!(self, Self::RedrawRequested { .. })
    }

    fn mouse_pressed(&self, btn: MouseButton) -> bool {
        match self {
            Self::MouseInput {
                state: ElementState::Pressed,
                button,
                ..
            } => button == &btn,
            _ => false,
        }
    }

    fn mouse_released(&self, btn: MouseButton) -> bool {
        match self {
            Self::MouseInput {
                state: ElementState::Released,
                button,
                ..
            } => button == &btn,
            _ => false,
        }
    }

    fn key_pressed(&self, key: Key<&str>) -> bool {
        match self {
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        state: ElementState::Pressed,
                        logical_key,
                        repeat: false,
                        ..
                    },
                ..
            } => logical_key.as_ref() == key,
            _ => false,
        }
    }

    fn key_released(&self, key: Key<&str>) -> bool {
        match self {
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        state: ElementState::Released,
                        logical_key,
                        repeat: false,
                        ..
                    },
                ..
            } => logical_key.as_ref() == key,
            _ => false,
        }
    }

    fn character_released(&self) -> Option<&str> {
        match self {
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        state: ElementState::Released,
                        logical_key: Key::Character(ch),
                        repeat: false,
                        ..
                    },
                ..
            } => Some(ch.as_str()),
            _ => None,
        }
    }
}
