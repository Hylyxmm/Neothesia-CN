use std::path::PathBuf;

use crate::{
    context::Context,
    scene::menu_scene::{MsgFn, on_async},
    song::Song,
    utils::BoxFuture,
};

use super::UiState;

pub fn open_midi_file_picker(data: &mut UiState, ctx: &mut Context) -> BoxFuture<MsgFn> {
    data.is_loading = true;
    // The native file dialog would be hidden behind exclusive fullscreen, so drop out
    // for the dialog and restore the persisted window mode once it closes.
    ctx.suspend_fullscreen();
    on_async(open_midi_file_picker_fut(), |res, data, ctx| {
        if let Some((midi, path)) = res {
            ctx.config.set_last_opened_song(Some(path));
            data.song = Some(Song::new(midi));
        }
        data.is_loading = false;
        ctx.restore_fullscreen();
    })
}

async fn open_midi_file_picker_fut() -> Option<(midi_file::MidiFile, PathBuf)> {
    let file = rfd::AsyncFileDialog::new()
        .add_filter("midi", &["mid", "midi"])
        .pick_file()
        .await;

    if let Some(file) = file {
        log::info!("File path = {:?}", file.path());

        let thread = crate::utils::task::thread::spawn("midi-loader".into(), move || {
            let midi = midi_file::MidiFile::new(file.path());

            if let Err(e) = &midi {
                log::error!("{e}");
            }

            midi.map(|midi| (midi, file.path().to_path_buf())).ok()
        });

        thread.join().await.ok().flatten()
    } else {
        log::info!("User canceled dialog");
        None
    }
}
