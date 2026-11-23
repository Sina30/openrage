//! The D16 target's install half: `cargo run -p or-title --example install`.

mod sample;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    sample::run()
}
