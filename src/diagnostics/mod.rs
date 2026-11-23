//! What the process knows, put where a user without a terminal can read it:
//! the in-memory log the Log panel shows, and the report a panic leaves.

pub mod crash;
pub mod log_buffer;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use tracing::{Level, Metadata};
use tracing_subscriber::filter::{filter_fn, EnvFilter, FilterExt};
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::{Layer, SubscriberExt};

use log_buffer::{BufferLayer, LogBuffer};

fn terminal_filter() -> EnvFilter {
    EnvFilter::from_default_env()
        .add_directive("openrage=debug".parse().expect("a static directive"))
        .add_directive("wgpu=warn".parse().expect("a static directive"))
}

/// ⚠ **Wider than the terminal's, never narrower.** `EnvFilter` with no
/// directive for a target admits errors only, so a warning from `or_title` or
/// `or_io` — most of what goes wrong opening a file — does not reach the
/// terminal. The panel takes everything the terminal shows, every warning
/// from anywhere, and `info` from the workspace's own crates.
fn panel_admits(meta: &Metadata<'_>) -> bool {
    let level = *meta.level();
    level <= Level::WARN || (level <= Level::INFO && meta.target().starts_with("or_"))
}

/// The subscriber the editor runs: one layer for the terminal, one for the
/// Log panel's buffer.
pub fn subscriber<W>(buffer: Arc<LogBuffer>, terminal: W) -> impl tracing::Subscriber + Send + Sync
where
    W: for<'w> MakeWriter<'w> + Send + Sync + 'static,
{
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(terminal)
                .with_filter(terminal_filter()),
        )
        .with(BufferLayer::new(buffer).with_filter(terminal_filter().or(filter_fn(panel_admits))))
}

/// Runs `body` with the application's own subscriber as the thread default, and returns what it logged.
///
/// ⚠ Tracing takes a lone dispatcher for the global default, so a callsite that a thread with no subscriber reaches first is cached as never wanted while another test's scoped one is the only dispatcher alive, and this capture then hears nothing. A permanent dispatcher ends that.
#[cfg(test)]
pub(crate) fn captured(body: impl FnOnce()) -> Arc<LogBuffer> {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let _ = tracing::subscriber::set_global_default(tracing_subscriber::registry());
    });
    let buffer = Arc::new(LogBuffer::default());
    tracing::subscriber::with_default(subscriber(Arc::clone(&buffer), std::io::sink), body);
    buffer
}

/// Logging and the panic hook, before anything else can fail.
pub fn install() {
    install_on(std::io::stdout);
}

/// [`install`] for a process whose stdout is a protocol: the terminal's log goes to stderr.
pub fn install_for_protocol() {
    install_on(std::io::stderr);
}

fn install_on<W>(terminal: W)
where
    W: for<'w> MakeWriter<'w> + Send + Sync + 'static,
{
    use tracing_subscriber::util::SubscriberInitExt;

    let buffer = Arc::clone(log_buffer::global());
    subscriber(Arc::clone(&buffer), terminal).init();
    crash::install(crash::Reporter {
        dir: crash::reports_dir(),
        buffer,
    });
    crash::restore_unread();
}
