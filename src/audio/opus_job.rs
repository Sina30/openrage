//! A GTA VI stream's Opus decoded to play or to export as WAV, or encoded anew from a WAV, on a
//! worker as a row of the job list with its progress and Cancel.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use or_io::awc::opus::OpusStream;
use or_io::awc::wav::{read_wav, wav_from_pcm16};
use or_io::awc::AwcFile;
use or_job::{Cancel, Progress};

use crate::state::jobs::{JobKind, Outcome, RunningJob, Then};
use crate::state::{AppState, DocumentId};

/// The rate every Opus stream decodes and encodes at.
const RATE: u32 = 48_000;

/// What the worker makes of a stream.
enum Work {
    /// Its samples, played, or written as a WAV at the path.
    Decode {
        groups: Vec<OpusStream>,
        wav: Option<PathBuf>,
    },
    /// The container with stream `stream` encoded anew from the WAV file at `from`.
    Encode {
        file: AwcFile,
        stream: usize,
        from: PathBuf,
    },
}

/// What the worker made.
enum Made {
    /// Interleaved samples and their channel count.
    Samples(Vec<i16>, u16),
    Written(PathBuf),
    /// The container with the stream replaced, and the WAV it was encoded from.
    Replaced(AwcFile, PathBuf),
}

/// One stream's decode or encode on a worker.
pub(super) struct OpusJob {
    doc: DocumentId,
    stream: usize,
    /// Where the document's history stood at the start, which a replace must still find.
    at: u64,
    doing: &'static str,
    cancel: Cancel,
    progress: Progress,
    ended: Arc<Mutex<Option<Result<Made, String>>>>,
}

impl OpusJob {
    fn start(doc: DocumentId, stream: usize, at: u64, doing: &'static str, work: Work) -> Self {
        let job = Self {
            doc,
            stream,
            at,
            doing,
            cancel: Cancel::new(),
            progress: Progress::new(),
            ended: Arc::default(),
        };
        let (cancel, progress, ended) = (
            job.cancel.clone(),
            job.progress.clone(),
            Arc::clone(&job.ended),
        );
        std::thread::spawn(move || {
            let watch = |done: u64, total: u64| {
                progress.reached(done, total);
                !cancel.is_set()
            };
            // A panic still ends the job, or the list shows it running for ever.
            let made = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(work, &watch)))
                .unwrap_or_else(|_| Err(tr!("It stopped on an internal error").to_string()));
            *ended.lock().unwrap_or_else(|p| p.into_inner()) = Some(made);
        });
        job
    }

    /// What the worker made, `None` while it runs.
    fn ended(&self) -> MutexGuard<'_, Option<Result<Made, String>>> {
        self.ended.lock().unwrap_or_else(|p| p.into_inner())
    }
}

impl Drop for OpusJob {
    fn drop(&mut self) {
        self.cancel.set();
    }
}

fn run(work: Work, watch: or_media::opus::Watch<'_>) -> Result<Made, String> {
    match work {
        Work::Decode { groups, wav } => {
            let pcm = or_media::awc::opus_pcm(&groups, watch).map_err(|e| e.to_string())?;
            let channels: u16 = groups.iter().map(|g| u16::from(g.channels)).sum();
            let Some(path) = wav else {
                return Ok(Made::Samples(pcm, channels));
            };
            let bytes: Vec<u8> = pcm.iter().flat_map(|s| s.to_le_bytes()).collect();
            std::fs::write(&path, wav_from_pcm16(&bytes, RATE, channels))
                .map_err(|e| tr!("Export failed: {e}", e = e))?;
            Ok(Made::Written(path))
        }
        Work::Encode { file, stream, from } => {
            let wav = std::fs::read(&from).map_err(|e| {
                tr!(
                    "Read error ({file_path}): {e}",
                    file_path = from.display(),
                    e = e
                )
            })?;
            let (pcm, rate, channels) = read_wav(&wav)?;
            let edited =
                or_media::opus::replaced_watched(&file, stream, &pcm, rate, channels, watch);
            Ok(Made::Replaced(edited.map_err(|e| e.to_string())?, from))
        }
    }
}

impl AppState {
    /// Plays Opus stream `stream` of `doc`, or writes it as a WAV at `wav`, decoded on a worker.
    pub fn awc_opus_decode(
        &mut self,
        doc: DocumentId,
        stream: usize,
        wav: Option<PathBuf>,
    ) -> Result<(), String> {
        let groups = or_media::awc::opus_groups(self.awc_file(doc)?, stream);
        let groups = groups.map_err(|e| e.to_string())?;
        let doing = if wav.is_some() {
            tr!("Decoding an Opus stream to WAV")
        } else {
            tr!("Decoding an Opus stream to play it")
        };
        self.start_opus(doc, stream, doing, Work::Decode { groups, wav });
        Ok(())
    }

    /// Replaces Opus stream `stream` of `doc` from the WAV at `from`, encoded on a worker.
    pub fn awc_opus_replace(
        &mut self,
        doc: DocumentId,
        stream: usize,
        from: PathBuf,
    ) -> Result<(), String> {
        self.awc_writable(doc)?;
        let file = self.awc_file(doc)?.clone();
        let doing = tr!("Encoding an Opus stream from a WAV");
        self.start_opus(doc, stream, doing, Work::Encode { file, stream, from });
        Ok(())
    }

    fn start_opus(&mut self, doc: DocumentId, stream: usize, doing: &'static str, work: Work) {
        let at = self.document(doc).map_or(0, |d| d.history.at());
        self.audio.opus = Some(OpusJob::start(doc, stream, at, doing, work));
    }

    /// The Opus job, while it runs, as the job list draws it.
    pub(crate) fn awc_opus_job(&self) -> Option<RunningJob> {
        let job = self.audio.opus.as_ref()?;
        if job.ended().is_some() {
            return None;
        }
        Some(RunningJob {
            kind: JobKind::OpusAudio,
            doing: job.doing.to_string(),
            fraction: Some(job.progress.snapshot().fraction()),
            detail: String::new(),
            stopping: job.cancel.is_set(),
        })
    }

    /// Asks the Opus job to stop; [`Self::poll_awc_opus`] says how it ended.
    pub(crate) fn stop_awc_opus(&self) {
        if let Some(job) = &self.audio.opus {
            job.cancel.set();
        }
    }

    /// Plays, reports or applies what the Opus job made, once it has ended.
    pub(crate) fn poll_awc_opus(&mut self) {
        let Some(made) = (self.audio.opus.as_ref()).and_then(|job| job.ended().take()) else {
            return;
        };
        let Some(job) = self.audio.opus.take() else {
            return;
        };
        let kind = JobKind::OpusAudio;
        if job.cancel.is_set() {
            let said = tr!("Stopped before it finished");
            self.jobs
                .record(kind, Outcome::Stopped(Then::Nothing), said);
            return;
        }
        let at = self.document(job.doc).map(|d| d.history.at());
        let ended = match made {
            Err(why) => Err(why),
            // A stream of a document closed since is not played.
            Ok(Made::Samples(..)) if at.is_none() => Ok(None),
            Ok(Made::Samples(pcm, channels)) => {
                self.audio.stop();
                let played = self.audio.sink.play(RATE, channels, pcm);
                if played.is_ok() {
                    self.audio.playing = Some((job.doc, job.stream));
                }
                played.map(|()| None)
            }
            Ok(Made::Written(path)) => {
                let folder = path.parent().map(Path::to_path_buf).unwrap_or_default();
                let said = tr!("Wrote {path}", path = path.display());
                Ok(Some((Then::OpenFolder(folder), said)))
            }
            // An edit made or undone while the worker ran is not dropped by the replace.
            Ok(Made::Replaced(..)) if at != Some(job.at) => {
                let why = tr!(
                    "The container changed while its stream was encoded, so the replace is dropped: replace it again"
                );
                Err(why.to_string())
            }
            Ok(Made::Replaced(edited, from)) => {
                let said = tr!("Replaced the stream from {path}", path = from.display());
                let replaced = self.awc_replaced(job.doc, job.stream, edited);
                replaced.map(|()| Some((Then::Show(job.doc), said)))
            }
        };
        match ended {
            Ok(None) => {}
            Ok(Some((then, said))) => {
                self.show_toast(said.clone());
                self.jobs.record(kind, Outcome::Done(then), said);
            }
            Err(why) => {
                self.show_error_toast(why.clone());
                self.jobs.record(kind, Outcome::Failed, why);
            }
        }
    }
}
