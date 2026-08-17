use std::path::PathBuf;

use crate::{
    context::Context,
    scene::menu_scene::{MsgFn, Popup, icons, neo_btn_icon, on_async},
    utils::BoxFuture,
};
use nuon::TextJustify;
use neothesia_core::config::FullscreenMode;
use piano_layout::Key;

use super::UiState;

fn button() -> nuon::Button {
    nuon::button()
        .color([74, 68, 88])
        .preseed_color([74, 68, 88])
        .hover_color([87, 81, 101])
        .border_radius([5.0; 4])
}

/// Audio gain slider range in dB; 0 dB (slider center) is unity gain.
const GAIN_DB_MIN: f32 = -18.0;
const GAIN_DB_MAX: f32 = 18.0;

/// Volume-bar style slider: rounded track, filled portion and a knob. Clicking or
/// dragging anywhere on the track sets the value; returns `Some(frac in 0..=1)` on the
/// frames the user is interacting with it. `center_notch` draws a small tick at the
/// middle of the track (useful when the center value is meaningful, e.g. 0 dB).
fn volume_slider(
    ui: &mut nuon::Ui,
    id: impl Into<nuon::Id>,
    x: f32,
    y_center: f32,
    w: f32,
    frac: f32,
    center_notch: bool,
) -> Option<f32> {
    let track_h = 6.0;
    let knob_d = 14.0;
    let track_y = y_center - track_h / 2.0;
    let frac = frac.clamp(0.0, 1.0);

    // Register the hit area first so the grab priority belongs to the slider.
    let event = nuon::click_area(id)
        .pos(x, y_center - 15.0)
        .size(w, 30.0)
        .build(ui);

    // Track
    nuon::quad()
        .pos(x, track_y)
        .size(w, track_h)
        .color([46, 42, 54, 255])
        .border_radius([track_h / 2.0; 4])
        .build(ui);

    // Fill from the center to the knob: the slider is a dB control where the middle
    // is unity — a negative value shows a short bar left of center, a positive one a
    // longer bar right of it.
    let center = 0.5_f32;
    let (fill_x, fill_w) = if frac < center {
        (x + w * frac, w * (center - frac))
    } else {
        (x + w * center, w * (frac - center))
    };
    if fill_w > 0.0 {
        nuon::quad()
            .pos(fill_x, track_y)
            .size(fill_w, track_h)
            .color([122, 104, 168, 255])
            .border_radius([track_h / 2.0; 4])
            .build(ui);
    }

    // Center notch
    if center_notch {
        nuon::quad()
            .pos(x + w * center - 1.0, y_center - track_h / 2.0 - 3.0)
            .size(2.0, track_h + 6.0)
            .color([90, 84, 104, 255])
            .build(ui);
    }

    // Knob
    nuon::quad()
        .pos(x + w * frac - knob_d / 2.0, y_center - knob_d / 2.0)
        .size(knob_d, knob_d)
        .color([220, 216, 228, 255])
        .border_radius([knob_d / 2.0; 4])
        .build(ui);

    if event.is_pressed() || event.is_press_start() {
        let cursor = ui.cursor_local();
        Some(((cursor.x - x) / w).clamp(0.0, 1.0))
    } else {
        None
    }
}

impl super::MenuScene {
    pub fn settings_page_ui(&mut self, ctx: &mut Context, ui: &mut nuon::Ui) {
        // Establish the selected output/input connection
        super::state::connect_io(&self.state, ctx);

        let win_w = ctx.window_state.logical_size.width;
        let win_h = ctx.window_state.logical_size.height;

        let bottom_bar_h = 60.0;

        nuon::translate().x(0.0).y(win_h).build(ui, |ui| {
            let padding = 10.0;
            let w = 80.0;
            let h = bottom_bar_h;

            // Bottom Margin
            nuon::translate().y(-padding).add_to_current(ui);
            nuon::translate().y(-h).add_to_current(ui);

            nuon::translate().x(0.0).build(ui, |ui| {
                nuon::translate().x(padding).add_to_current(ui);

                if neo_btn_icon(ui, w, h, icons::left_arrow_icon()) {
                    self.state.go_back();
                }

                nuon::translate().x(-w - padding).add_to_current(ui);
            });
        });

        let margin_top = 40.0;
        let body_w = 650.0;

        self.settings_scroll = nuon::scroll()
            .scissor_size(win_w, (win_h - bottom_bar_h).max(0.0))
            .scroll(self.settings_scroll)
            .build(ui, |ui| {
                nuon::translate()
                    .x(nuon::center_x(win_w, body_w))
                    .add_to_current(ui);
                nuon::translate().y(margin_top).add_to_current(ui);

                nuon::settings_section("显示")
                    .width(body_w)
                    .build(ui, |ui, rows, spacer| {
                        if nuon::settings_row_toggler()
                            .title("全屏")
                            .subtitle("在所选显示器上全屏运行")
                            .value(ctx.config.fullscreen())
                            .build(ui, rows)
                        {
                            ctx.config.set_fullscreen(!ctx.config.fullscreen());
                            ctx.apply_window_settings();
                        }

                        spacer(ui);

                        nuon::settings_row()
                            .title("全屏模式")
                            .subtitle("无边框不切换显示模式，避免黑屏闪烁；独占可自选分辨率")
                            .body(|ui, row_w, row_h| {
                                let w = 110.0;
                                let h = 31.0;
                                let borderless =
                                    ctx.config.fullscreen_mode() == FullscreenMode::Borderless;
                                if button()
                                    .x(row_w - w)
                                    .y(nuon::center_y(row_h, h))
                                    .size(w, h)
                                    .label(if borderless { "无边框" } else { "独占" })
                                    .build(ui)
                                {
                                    ctx.config.set_fullscreen_mode(if borderless {
                                        FullscreenMode::Exclusive
                                    } else {
                                        FullscreenMode::Borderless
                                    });
                                    // Switch immediately if currently fullscreen.
                                    ctx.apply_window_settings();
                                }
                            })
                            .build(ui, rows);

                        spacer(ui);

                        nuon::settings_row()
                            .title("显示器")
                            .subtitle("全屏时使用的屏幕")
                            .body(|ui, row_w, row_h| {
                                self.settings_monitor_picker(ui, ctx, row_w, row_h)
                            })
                            .build(ui, rows);

                        spacer(ui);

                        nuon::settings_row()
                            .title("分辨率")
                            .subtitle("无边框全屏跟随桌面；窗口模式下为窗口大小")
                            .body(|ui, row_w, row_h| {
                                self.settings_resolution_picker(ui, ctx, row_w, row_h)
                            })
                            .build(ui, rows);
                    });

                nuon::settings_section("音频输出")
                    .width(body_w)
                    .build(ui, |ui, rows, spacer| {
                        self.settings_output_section(ctx, ui, rows, spacer);
                    });

                nuon::settings_section("MIDI 输入")
                    .width(body_w)
                    .build(ui, |ui, rows, spacer| {
                        self.settings_input_section(ctx, ui, rows, spacer);
                    });

                nuon::settings_section("音符范围")
                    .width(body_w)
                    .build(ui, |ui, rows, spacer| {
                        self::update_range_start(
                            ctx,
                            nuon::settings_row_spin()
                                .title("起始")
                                .subtitle(ctx.config.piano_range().start().to_string())
                                .id("range-start")
                                .build(ui, rows),
                        );

                        spacer(ui);

                        self::update_range_end(
                            ctx,
                            nuon::settings_row_spin()
                                .title("结束")
                                .subtitle(ctx.config.piano_range().end().to_string())
                                .id("range-end")
                                .build(ui, rows),
                        );

                        spacer(ui);

                        // 重置为标准 88 键音域
                        nuon::settings_row()
                            .title("默认音域")
                            .subtitle("重置为标准 88 键 (A0–C8)")
                            .body(|ui, row_w, row_h| {
                                let w = 93.0;
                                let h = 31.0;
                                if button()
                                    .x(row_w - w)
                                    .y(nuon::center_y(row_h, h))
                                    .size(w, h)
                                    .label("重置")
                                    .build(ui)
                                {
                                    // 标准 88 键音域：A0(21) – C8(108)
                                    ctx.config.set_piano_range_start(21);
                                    ctx.config.set_piano_range_end(108);
                                }
                            })
                            .build(ui, rows);
                    });

                nuon::translate().y(10.0).add_to_current(ui);

                let keyboard_h = 100.0;
                self.keyboard_layout_preview(ctx, body_w, keyboard_h, ui);
                nuon::translate().y(keyboard_h).add_to_current(ui);

                nuon::settings_section("渲染")
                    .width(body_w)
                    .build(ui, |ui, rows, spacer| {
                        self::update_flow_speed(
                            ctx,
                            nuon::settings_row_spin()
                                .title("瀑布流速")
                                .subtitle(ctx.config.animation_speed().round().to_string())
                                .id("flow-speed")
                                .build(ui, rows),
                        );

                        spacer(ui);

                        if nuon::settings_row_toggler()
                            .title("垂直辅助线")
                            .subtitle("显示八度标记")
                            .value(ctx.config.vertical_guidelines())
                            .build(ui, rows)
                        {
                            ctx.config
                                .set_vertical_guidelines(!ctx.config.vertical_guidelines());
                        }

                        spacer(ui);

                        if nuon::settings_row_toggler()
                            .title("水平辅助线")
                            .subtitle("显示小节线")
                            .value(ctx.config.horizontal_guidelines())
                            .build(ui, rows)
                        {
                            ctx.config
                                .set_horizontal_guidelines(!ctx.config.horizontal_guidelines());
                        }

                        spacer(ui);

                        if nuon::settings_row_toggler()
                            .title("发光")
                            .subtitle("琴键发光效果")
                            .value(ctx.config.glow())
                            .build(ui, rows)
                        {
                            ctx.config.set_glow(!ctx.config.glow());
                        }

                        spacer(ui);

                        if nuon::settings_row_toggler()
                            .title("音符标签")
                            .subtitle("显示瀑布音符标签")
                            .value(ctx.config.note_labels())
                            .build(ui, rows)
                        {
                            ctx.config.set_note_labels(!ctx.config.note_labels());
                        }

                        spacer(ui);

                        if nuon::settings_row_toggler()
                            .title("隐藏键盘")
                            .subtitle("瀑布延伸到屏幕底部(外接钢琴场景)")
                            .value(ctx.config.hide_keyboard())
                            .build(ui, rows)
                        {
                            ctx.config.set_hide_keyboard(!ctx.config.hide_keyboard());
                        }
                    });
            });
    }
}

impl super::MenuScene {
    fn settings_output_picker(
        &mut self,
        ui: &mut nuon::Ui,
        ctx: &mut Context,
        row_w: f32,
        row_h: f32,
    ) {
        let btn_w = 320.0;
        let btn_h = 31.0;

        let btn_x = row_w - btn_w;
        let btn_y = nuon::center_y(row_h, btn_h);

        if button()
            .pos(btn_x, btn_y)
            .size(btn_w, btn_h)
            .id("select_output")
            .label(
                self.state
                    .selected_output
                    .as_ref()
                    .map(|o| o.to_string())
                    .unwrap_or_default(),
            )
            .text_justify(TextJustify::Left)
            .build(ui)
        {
            self.popup.toggle(Popup::OutputSelector);
        }

        nuon::label()
            .icon(icons::caret_down())
            .pos(btn_x, btn_y)
            .size(btn_w, btn_h)
            .text_justify(TextJustify::Right)
            .build(ui);

        if self.popup == Popup::OutputSelector {
            nuon::layer().overlay(true).build(ui, |ui| {
                nuon::translate()
                    .x(btn_x)
                    .y(btn_y + btn_h)
                    .add_to_current(ui);

                let data = &mut self.state;

                if let Some(output) =
                    nuon::combo_list(ui, "select_output_", (btn_w, btn_h), &data.outputs)
                {
                    ctx.config
                        .set_output(output.is_not_dummy().then(|| output.to_string()));
                    data.selected_output = Some(output.clone());
                    self.popup.close();
                }
            });
        }
    }

    fn settings_output_section(
        &mut self,
        ctx: &mut Context,
        ui: &mut nuon::Ui,
        rows: &dyn Fn(&mut nuon::Ui, nuon::SettingsRow<'_>),
        spacer: &dyn Fn(&mut nuon::Ui),
    ) {
        nuon::settings_row()
            .title("音频输出")
            .body(|ui, row_w, row_h| self.settings_output_picker(ui, ctx, row_w, row_h))
            .build(ui, rows);

        let (is_synth, is_midi) = self
            .state
            .selected_output
            .as_ref()
            .map(|o| (o.is_synth(), o.is_midi()))
            .unwrap_or((false, false));

        if is_synth {
            spacer(ui);

            nuon::settings_row()
                .title("音色库")
                .subtitle(
                    ctx.config
                        .soundfont_path()
                        .as_ref()
                        .and_then(|path| path.file_name())
                        .map(|name| name.to_string_lossy().to_string())
                        .unwrap_or_default(),
                )
                .body(|ui, row_w, row_h| {
                    let w = 93.0;
                    let h = 31.0;
                    if button()
                        .x(row_w - w)
                        .y(nuon::center_y(row_h, h))
                        .size(w, h)
                        .label("选择文件")
                        .build(ui)
                    {
                        self.futures
                            .push(self::open_soundfont_picker(&mut self.state, ctx));
                    }
                })
                .build(ui, rows);

            spacer(ui);

            // Slider maps [-18 dB .. +18 dB] to the synth's linear gain (10^(dB/20)),
            // 0 dB at the center. Stored gain values outside the range just clamp.
            let db = (20.0 * ctx.config.audio_gain().log10()).clamp(GAIN_DB_MIN, GAIN_DB_MAX);
            let label = if db.abs() < 0.05 {
                "0.0 dB".to_string()
            } else {
                format!("{db:+.1} dB")
            };
            nuon::settings_row()
                .title("音频增益")
                .subtitle(label)
                .body(|ui, row_w, row_h| {
                    let w = 200.0;
                    let frac = (db - GAIN_DB_MIN) / (GAIN_DB_MAX - GAIN_DB_MIN);
                    if let Some(f) =
                        volume_slider(ui, "gain-slider", row_w - w, row_h / 2.0, w, frac, true)
                    {
                        let db =
                            ((GAIN_DB_MIN + f * (GAIN_DB_MAX - GAIN_DB_MIN)) * 10.0).round()
                                / 10.0;
                        ctx.config.set_audio_gain(10f32.powf(db / 20.0));
                    }
                })
                .build(ui, rows);
        } else if is_midi {
            spacer(ui);

            if nuon::settings_row_toggler()
                .title("分离通道")
                .subtitle("为每个音轨分配不同 MIDI 通道")
                .value(ctx.config.separate_channels())
                .build(ui, rows)
            {
                ctx.config
                    .set_separate_channels(!ctx.config.separate_channels());
            }
        }
    }
}

impl super::MenuScene {
    fn settings_input_picker(
        &mut self,
        ui: &mut nuon::Ui,
        ctx: &mut Context,
        row_w: f32,
        row_h: f32,
    ) {
        let btn_w = 320.0;
        let btn_h = 31.0;

        let btn_x = row_w - btn_w;
        let btn_y = nuon::center_y(row_h, btn_h);

        if button()
            .pos(btn_x, btn_y)
            .size(btn_w, btn_h)
            .id("select_input")
            .label(
                self.state
                    .selected_input
                    .as_ref()
                    .map(|o| o.to_string())
                    .unwrap_or_default(),
            )
            .text_justify(TextJustify::Left)
            .build(ui)
        {
            self.popup.toggle(Popup::InputSelector);
        }

        nuon::label()
            .icon(icons::caret_down())
            .pos(btn_x, btn_y)
            .size(btn_w, btn_h)
            .text_justify(TextJustify::Right)
            .build(ui);

        if self.popup == Popup::InputSelector {
            nuon::layer().overlay(true).build(ui, |ui| {
                nuon::translate()
                    .x(btn_x)
                    .y(btn_y + btn_h)
                    .add_to_current(ui);

                let data = &mut self.state;

                if let Some(input) =
                    nuon::combo_list(ui, "select_input_", (btn_w, btn_h), &data.inputs)
                {
                    ctx.config.set_input(Some(&input));
                    data.selected_input = Some(input.clone());
                    self.popup.close();
                }
            });
        }
    }

    fn settings_input_section(
        &mut self,
        ctx: &mut Context,
        ui: &mut nuon::Ui,
        rows: &dyn Fn(&mut nuon::Ui, nuon::SettingsRow<'_>),
        spacer: &dyn Fn(&mut nuon::Ui),
    ) {
        nuon::settings_row()
            .title("MIDI 输入")
            .body(|ui, row_w, row_h| self.settings_input_picker(ui, ctx, row_w, row_h))
            .build(ui, rows);

        spacer(ui);

        if nuon::settings_row_toggler()
            .title("键盘输入静音")
            .subtitle("输入只做对错判定，不输出软件音源")
            .value(ctx.config.mute_user_input())
            .build(ui, rows)
        {
            ctx.config.set_mute_user_input(!ctx.config.mute_user_input());
        }
    }
}

impl super::MenuScene {
    fn settings_monitor_picker(
        &mut self,
        ui: &mut nuon::Ui,
        ctx: &mut Context,
        row_w: f32,
        row_h: f32,
    ) {
        let btn_w = 320.0;
        let btn_h = 31.0;

        let btn_x = row_w - btn_w;
        let btn_y = nuon::center_y(row_h, btn_h);

        let monitors = crate::utils::window::list_monitors(&ctx.window);
        let current_label = ctx
            .config
            .monitor()
            .and_then(|name| monitors.iter().find(|m| m.name == *name))
            .map(|m| m.label.clone())
            .unwrap_or_else(|| "跟随当前显示器".to_string());

        if button()
            .pos(btn_x, btn_y)
            .size(btn_w, btn_h)
            .id("select_monitor")
            .label(current_label)
            .text_justify(TextJustify::Left)
            .build(ui)
        {
            self.popup.toggle(Popup::MonitorSelector);
        }

        nuon::label()
            .icon(icons::caret_down())
            .pos(btn_x, btn_y)
            .size(btn_w, btn_h)
            .text_justify(TextJustify::Right)
            .build(ui);

        if self.popup == Popup::MonitorSelector {
            nuon::layer().overlay(true).build(ui, |ui| {
                nuon::translate()
                    .x(btn_x)
                    .y(btn_y + btn_h)
                    .add_to_current(ui);

                let labels: Vec<String> = monitors.iter().map(|m| m.label.clone()).collect();

                if let Some(selected) =
                    nuon::combo_list(ui, "select_monitor_", (btn_w, btn_h), &labels)
                {
                    if let Some(m) = monitors.iter().find(|m| &m.label == selected) {
                        ctx.config.set_monitor(Some(m.name.clone()));
                        // Re-apply so an active fullscreen switches to the new monitor.
                        ctx.apply_window_settings();
                    }
                    self.popup.close();
                }
            });
        }
    }

    fn settings_resolution_picker(
        &mut self,
        ui: &mut nuon::Ui,
        ctx: &mut Context,
        row_w: f32,
        row_h: f32,
    ) {
        let btn_w = 320.0;
        let btn_h = 31.0;

        let btn_x = row_w - btn_w;
        let btn_y = nuon::center_y(row_h, btn_h);

        let modes = crate::utils::window::list_resolutions(
            &ctx.window,
            ctx.config.monitor().map(|s| s.as_str()),
        );
        let current_label = ctx
            .config
            .resolution()
            .and_then(|r| modes.iter().find(|m| m.size == r).map(|m| m.label()))
            .unwrap_or_else(|| "跟随显示器".to_string());

        if button()
            .pos(btn_x, btn_y)
            .size(btn_w, btn_h)
            .id("select_resolution")
            .label(current_label)
            .text_justify(TextJustify::Left)
            .build(ui)
        {
            self.popup.toggle(Popup::ResolutionSelector);
        }

        nuon::label()
            .icon(icons::caret_down())
            .pos(btn_x, btn_y)
            .size(btn_w, btn_h)
            .text_justify(TextJustify::Right)
            .build(ui);

        if self.popup == Popup::ResolutionSelector {
            nuon::layer().overlay(true).build(ui, |ui| {
                nuon::translate()
                    .x(btn_x)
                    .y(btn_y + btn_h)
                    .add_to_current(ui);

                let labels: Vec<String> = modes.iter().map(|m| m.label()).collect();

                if let Some(selected) =
                    nuon::combo_list(ui, "select_resolution_", (btn_w, btn_h), &labels)
                {
                    if let Some(m) = modes.iter().find(|m| &m.label() == selected) {
                        ctx.config.set_resolution(Some(m.size));
                        // Switches the exclusive-fullscreen mode, or the windowed size.
                        ctx.apply_window_settings();
                    }
                    self.popup.close();
                }
            });
        }
    }
}

impl super::MenuScene {
    fn keyboard_layout_preview(
        &mut self,
        ctx: &Context,
        keyboard_w: f32,
        keyboard_h: f32,
        ui: &mut nuon::Ui,
    ) {
        nuon::quad()
            .size(keyboard_w, keyboard_h)
            .color([255; 3])
            .border_radius([7.0; 4])
            .build(ui);

        let range = piano_layout::KeyboardRange::new(ctx.config.piano_range());

        let white_count = range.white_count();
        let neutral_width = keyboard_w / white_count as f32;
        let neutral_height = keyboard_h;

        let layout = piano_layout::KeyboardLayout::from_range(
            piano_layout::Sizing::new(neutral_width, neutral_height),
            range,
        );

        let mut build_key = |ui: &mut nuon::Ui, key: &Key| {
            let note = layout.range.start() + key.id() as u8;
            let x = key.x();
            let y = 0.0;
            let width = key.width();
            let height = key.height();

            let event = nuon::click_area(format!("settings-preview-key-{note}"))
                .pos(key.x(), 0.0)
                .size(key.width(), key.height())
                .build(ui);

            if event.is_press_start() {
                ctx.output_manager.connection().midi_event(
                    0.into(),
                    midi_file::midly::MidiMessage::NoteOn {
                        key: note.into(),
                        vel: 100.into(),
                    },
                );
                self.midi_input_state.note_on(note);
            }
            if event.is_press_end() {
                ctx.output_manager.connection().midi_event(
                    0.into(),
                    midi_file::midly::MidiMessage::NoteOff {
                        key: note.into(),
                        vel: 0.into(),
                    },
                );
                self.midi_input_state.note_off(note);
            }

            nuon::quad()
                .pos(x, y)
                .size(width, height)
                .color(if self.midi_input_state.is_pressed(note) {
                    [122, 104, 168, 255]
                } else if key.kind().is_sharp() {
                    [0, 0, 0, 255]
                } else {
                    [0; 4]
                })
                .build(ui);
        };

        nuon::layer().build(ui, |ui| {
            for key in layout.keys.iter().filter(|key| key.kind().is_sharp()) {
                build_key(ui, key);
            }
        });

        for key in layout.keys.iter().filter(|key| key.kind().is_neutral()) {
            build_key(ui, key);
        }

        // Key borders
        let mut neutral = layout
            .keys
            .iter()
            .filter(|key| key.kind().is_neutral())
            .peekable();

        while let Some(key) = neutral.next() {
            if neutral.peek().is_some() {
                nuon::quad()
                    .x(key.x() + key.width())
                    .y(0.0)
                    .size(1.0, key.height())
                    .color([150; 3])
                    .build(ui);
            }
        }
    }
}

/// Waterfall flow speed: how fast note blocks scroll down. Independent of the playback
/// speed multiplier — a higher speed stretches each block (more pixels per second of
/// note duration) so fewer blocks fit on screen; the song tempo is untouched. During
/// playback the same value is live-adjustable with PageUp/PageDown.
pub fn update_flow_speed(ctx: &mut Context, kind: nuon::SettingsRowSpinResult) {
    // Step of 100 px/s matches the PageUp/PageDown increment; 0.0 is rejected by the
    // config (it flips the sign instead), so jump over it, and keep a positive floor.
    const STEP: f32 = 100.0;
    match kind {
        nuon::SettingsRowSpinResult::Plus => {
            let mut v = ctx.config.animation_speed() + STEP;
            if v == 0.0 {
                v = STEP;
            }
            ctx.config.set_animation_speed(v.min(3000.0));
        }
        nuon::SettingsRowSpinResult::Minus => {
            let v = ctx.config.animation_speed() - STEP;
            if v >= 50.0 {
                ctx.config.set_animation_speed(v);
            }
        }
        nuon::SettingsRowSpinResult::Idle => {}
    }
}

pub fn update_range_start(ctx: &mut Context, kind: nuon::SettingsRowSpinResult) {
    match kind {
        nuon::SettingsRowSpinResult::Plus => {
            let v = (ctx.config.piano_range().start() + 1).min(127);
            if v + 24 < *ctx.config.piano_range().end() {
                ctx.config.set_piano_range_start(v);
            }
        }
        nuon::SettingsRowSpinResult::Minus => {
            ctx.config
                .set_piano_range_start(ctx.config.piano_range().start().saturating_sub(1));
        }
        nuon::SettingsRowSpinResult::Idle => {}
    }
}

pub fn update_range_end(ctx: &mut Context, kind: nuon::SettingsRowSpinResult) {
    match kind {
        nuon::SettingsRowSpinResult::Plus => {
            ctx.config
                .set_piano_range_end(ctx.config.piano_range().end() + 1);
        }
        nuon::SettingsRowSpinResult::Minus => {
            let v = ctx.config.piano_range().end().saturating_sub(1);
            if *ctx.config.piano_range().start() + 24 < v {
                ctx.config.set_piano_range_end(v);
            }
        }
        nuon::SettingsRowSpinResult::Idle => {}
    }
}

pub fn open_soundfont_picker(data: &mut UiState, ctx: &mut Context) -> BoxFuture<MsgFn> {
    data.is_loading = true;
    // Drop out of fullscreen for the native dialog, restore afterwards (same rationale
    // as the MIDI file picker).
    ctx.suspend_fullscreen();
    on_async(open_sondfont_picker_fut(), |res, data, ctx| {
        if let Some(font) = res {
            ctx.config.set_soundfont_path(Some(font.clone()));
        }
        data.is_loading = false;
        ctx.restore_fullscreen();
    })
}

async fn open_sondfont_picker_fut() -> Option<PathBuf> {
    let file = rfd::AsyncFileDialog::new()
        .add_filter("SoundFont2", &["sf2"])
        .pick_file()
        .await;

    if let Some(file) = file.as_ref() {
        log::info!("Font path = {:?}", file.path());
    } else {
        log::info!("User canceled dialog");
    }

    file.map(|f| f.path().to_owned())
}
