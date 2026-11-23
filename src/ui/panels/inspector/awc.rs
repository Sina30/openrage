use super::*;

enum Action {
    Play(usize),
    Stop,
    Export(usize, String),
    ExportOgg(usize),
    Replace(usize),
    OpenAnimation(usize),
    PutAnimation(usize),
}

pub(super) fn draw_awc(ui: &mut egui::Ui, state: &mut AppState, doc_id: DocumentId) {
    ui.heading("Audio Wave Container");
    ui.separator();
    let rows = match state.awc_streams(doc_id) {
        Ok(rows) => rows,
        Err(why) => {
            ui.colored_label(theme::WARNING, why);
            return;
        }
    };
    let playing = state.audio.playing();
    if playing.is_some() {
        // Repaints until the sink runs dry, so the button turns back to Play.
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(200));
    }

    if let Some((count, why)) = state.awc_refused(doc_id, &rows) {
        ui.colored_label(
            theme::WARNING,
            tr_n!(
                "{count} stream is not listed, because OpenRage refuses its Opus: {why}",
                "{count} streams are not listed, because OpenRage refuses their Opus. The first: {why}",
                count,
                why = why
            ),
        );
    }

    // ⚠ Only the rows in view: a stated table may hold a hundred thousand streams.
    let lines: Vec<_> = (rows.iter())
        .flat_map(|row| {
            std::iter::once((row, None))
                .chain(row.opus_channels.iter().map(move |c| (row, Some(c))))
        })
        .collect();
    // A stream a radio or a dialogue view asked to hear plays once its container has opened.
    let asked = (state.document(doc_id).and_then(|d| d.path.as_deref()))
        .and_then(|path| crate::state::gta6_dialogue::play_asked(path, &rows));
    let mut action = asked.map(Action::Play);
    egui::ScrollArea::both()
        .id_salt(("awc_streams", doc_id))
        .max_height(ui.clip_rect().height())
        .auto_shrink([true, true])
        .show_rows(ui, ui.spacing().interact_size.y, lines.len() + 1, |ui, range| {
            egui::Grid::new("awc_streams").striped(true).show(ui, |ui| {
                for heading in ["Stream", "Codec", "Channels", "Rate", "Length", ""] {
                    ui.strong(heading);
                }
                ui.end_row();
                for &(row, channel) in lines.iter().skip(range.start).take(range.len()) {
                    if let Some((id, channels, samples)) = channel {
                        ui.monospace(format!("  0x{id:08X}"));
                        ui.label(row.codec.label());
                        ui.label(channels.to_string());
                        ui.label(format!("{} Hz", row.sample_rate));
                        let seconds = f64::from(*samples) / f64::from(row.sample_rate.max(1));
                        ui.label(length(row.codec, seconds, *samples, None));
                        ui.end_row();
                        continue;
                    }
                    ui.monospace(row.name());
                    ui.label(row.codec.label());
                    ui.label(row.channels.to_string());
                    ui.label(format!("{} Hz", row.sample_rate));
                    ui.label(length(row.codec, row.seconds(), row.samples, row.loop_point));
                    ui.horizontal(|ui| {
                        let opus = row.codec == or_media::Codec::Opus;
                        if playing == Some((doc_id, row.stream)) {
                            if ui.button(format!("{} Stop", ph::STOP)).clicked() {
                                action = Some(Action::Stop);
                            }
                        } else if ui.button(format!("{} Play", ph::PLAY)).clicked() {
                            action = Some(Action::Play(row.stream));
                        }
                        if opus
                            && ui
                                .button(format!("{} Export Ogg Opus", ph::EXPORT))
                                .on_hover_text(tr!(
                                    "Writes this stream's Opus packets, one Ogg Opus file per stream or channel, named for its id, into a folder. Each file states a pre-skip of {skip} samples that is inferred, not stated by the file, and a multi-channel source's files leave out the packets each block repeats from the block before",
                                    skip = or_io::awc::ogg::PRE_SKIP
                                ))
                                .clicked()
                        {
                            action = Some(Action::ExportOgg(row.stream));
                        }
                        if ui.button(format!("{} Export WAV", ph::EXPORT)).clicked() {
                            action = Some(Action::Export(row.stream, row.name()));
                        }
                        if ui
                            .button(format!("{} Replace", ph::SWAP))
                            .on_hover_text("Replace this stream's samples from a 16-bit PCM WAV")
                            .clicked()
                        {
                            action = Some(Action::Replace(row.stream));
                        }
                        if row.animation
                            && ui
                                .button(format!("{} {}", ph::PERSON_SIMPLE_WALK, tr!("Open its animation")))
                                .on_hover_text(tr!("Opens the GTA VI animation resource this stream stores beside its audio: its clips, animations and tracks, read-only"))
                                .clicked()
                        {
                            action = Some(Action::OpenAnimation(row.stream));
                        }
                        if row.animation
                            && ui
                                .button(format!("{} {}", ph::ARROW_LEFT, tr!("Write its animation back")))
                                .on_hover_text(tr!("Writes the edited animation this stream's animation document holds back as the stream's animation chunk, a step of undo; Save writes the container"))
                                .clicked()
                        {
                            action = Some(Action::PutAnimation(row.stream));
                        }
                    });
                    ui.end_row();
                }
            });
        });

    // An Opus stream decodes and encodes on a worker, listed with the other jobs.
    let opus = |stream: usize| {
        (rows.iter()).any(|r| r.stream == stream && r.codec == or_media::Codec::Opus)
    };
    let outcome = match action {
        None => return,
        Some(Action::Stop) => {
            state.audio.stop();
            return;
        }
        Some(Action::Play(stream)) if opus(stream) => {
            state.awc_opus_decode(doc_id, stream, None).map(|()| None)
        }
        Some(Action::Play(stream)) => state.awc_play(doc_id, stream).map(|()| None),
        Some(Action::Export(stream, name)) => export(state, doc_id, stream, &name, opus(stream)),
        Some(Action::ExportOgg(stream)) => export_ogg(state, doc_id, stream),
        Some(Action::Replace(stream)) => replace(state, doc_id, stream, opus(stream)),
        Some(Action::OpenAnimation(stream)) => {
            state.awc_open_animation(doc_id, stream).map(|()| None)
        }
        Some(Action::PutAnimation(stream)) => {
            state.awc_put_animation(doc_id, stream).map(|()| None)
        }
    };
    match outcome {
        Ok(Some(done)) => state.show_toast(done),
        Ok(None) => {}
        Err(why) => state.show_error_toast(why),
    }
}

/// A stream's length; an Opus stream's also says its samples per channel and its record's loop point.
fn length(codec: or_media::Codec, seconds: f64, samples: u32, loop_point: Option<u32>) -> String {
    let seconds = format!("{seconds:.2}");
    match loop_point {
        _ if codec != or_media::Codec::Opus => format!("{seconds} s"),
        None => tr!(
            "{seconds} s, {samples} samples",
            seconds = seconds,
            samples = samples
        ),
        Some(at) => tr!(
            "{seconds} s, {samples} samples, loop point {at} (a role inferred from its values)",
            seconds = seconds,
            samples = samples,
            at = at
        ),
    }
}

fn export(
    state: &mut AppState,
    doc_id: DocumentId,
    stream: usize,
    name: &str,
    opus: bool,
) -> Result<Option<String>, String> {
    let wav = if opus {
        None
    } else {
        Some(state.awc_wav(doc_id, stream)?)
    };
    let Some(path) = crate::platform::file_dialogs::ask("Export Stream as WAV", |dialog| {
        dialog
            .set_file_name(format!("{name}.wav"))
            .add_filter("WAV", &["wav"])
            .save_file()
    }) else {
        return Ok(None);
    };
    let Some(wav) = wav else {
        state.awc_opus_decode(doc_id, stream, Some(path))?;
        return Ok(None);
    };
    std::fs::write(&path, wav).map_err(|e| format!("Export failed: {e}"))?;
    Ok(Some(format!("Wrote {}", path.display())))
}

/// Writes a GTA VI stream's Ogg Opus files into a folder the person picks.
fn export_ogg(
    state: &mut AppState,
    doc_id: DocumentId,
    stream: usize,
) -> Result<Option<String>, String> {
    let files = state.awc_ogg_opus(doc_id, stream)?;
    let Some(dir) = crate::platform::file_dialogs::pick_folder("Export Stream as Ogg Opus") else {
        return Ok(None);
    };
    for (name, bytes) in &files {
        std::fs::write(dir.join(name), bytes).map_err(|e| tr!("Export failed: {e}", e = e))?;
    }
    Ok(Some(tr_n!(
        "Wrote {count} Ogg Opus file to {dir}, with a pre-skip of {skip} samples that is inferred, not stated by the file",
        "Wrote {count} Ogg Opus files to {dir}, with a pre-skip of {skip} samples that is inferred, not stated by the file",
        files.len(),
        dir = dir.display(),
        skip = or_io::awc::ogg::PRE_SKIP
    )))
}

fn replace(
    state: &mut AppState,
    doc_id: DocumentId,
    stream: usize,
    opus: bool,
) -> Result<Option<String>, String> {
    let Some(path) = crate::platform::file_dialogs::ask("Replace Stream from WAV", |dialog| {
        dialog.add_filter("WAV", &["wav"]).pick_file()
    }) else {
        return Ok(None);
    };
    if opus {
        state.awc_opus_replace(doc_id, stream, path)?;
        return Ok(None);
    }
    let wav = std::fs::read(&path).map_err(|e| format!("Read error: {e}"))?;
    state.awc_replace(doc_id, stream, &wav)?;
    Ok(Some(format!("Replaced the stream from {}", path.display())))
}
