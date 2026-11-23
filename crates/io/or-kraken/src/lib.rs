//! Oodle Kraken, decoded by a pure-Rust port of ooz.
//!
//! ⚠ **This is a crate of its own for its panics.** The port indexes slices on a stream's say-so and
//! panics on one it cannot read. A panic unwinds only through frames compiled to unwind, and the
//! development profile compiles most of the workspace with Cranelift, which does not, so the catch
//! lives here, in a crate `.cargo/config.toml` compiles with LLVM, and nothing above it sees a panic.
//!
//! ⚠ **A quantum is decoded on its own.** A stream is quanta of 0x40000 output bytes, each led by a
//! two-byte block header and a three-byte size, and each begins with eight literal bytes, as the first
//! does. The port copies those eight only at the start of the whole stream, so a stream of more than
//! one quantum fails at the second; cutting the stream into quanta and giving the port each as a
//! stream of its own is what makes them decode.

const QUANTUM: usize = 0x40000;

/// A Kraken stream of exactly `wanted` bytes, or why it could not be read.
pub fn decode(stream: &[u8], wanted: usize) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(wanted);
    let mut at = 0usize;
    while out.len() < wanted {
        let want = (wanted - out.len()).min(QUANTUM);
        let rest = stream.get(at..).unwrap_or_default();
        let length = quantum_length(rest, want)?;
        let piece = rest
            .get(..length)
            .ok_or_else(|| "the stream ends early".to_string())?;
        out.extend_from_slice(&quantum(piece, want)?);
        at += length;
    }
    Ok(out)
}

/// How many bytes of `stream` the next quantum takes: its block header, its size and its body.
fn quantum_length(stream: &[u8], want: usize) -> Result<usize, String> {
    let [b0, b1, rest @ ..] = stream else {
        return Err("the stream ends early".into());
    };
    if b0 & 0xF != 0xC || (b0 >> 4) & 3 != 0 {
        return Err(format!("not a block header: {b0:02x} {b1:02x}"));
    }
    if b1 & 0x80 != 0 {
        return Err("a block with checksums".into());
    }
    if b0 & 0x40 != 0 {
        return Ok(2 + want);
    }
    let [s0, s1, s2, ..] = rest else {
        return Err("the stream ends early".into());
    };
    let v = usize::from(*s0) << 16 | usize::from(*s1) << 8 | usize::from(*s2);
    match v & 0x3FFFF {
        0x3FFFF if v >> 18 == 1 => Ok(2 + 3 + 1),
        0x3FFFF => Err("a quantum header that is neither compressed nor a fill".into()),
        size => Ok(2 + 3 + size + 1),
    }
}

fn quantum(piece: &[u8], want: usize) -> Result<Vec<u8>, String> {
    let mut out = vec![0u8; want];
    let decoded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        oozextract::Extractor::new().read_from_slice(piece, &mut out)
    }))
    .map_err(|_| "the Kraken decoder gave up on this stream".to_string())?;
    let got = decoded.map_err(|e| format!("{e:?}"))?;
    if got != want {
        return Err(format!(
            "Kraken gave {got} bytes of the {want} a quantum holds"
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::decode;

    #[test]
    fn a_stream_that_is_not_kraken_is_refused_and_never_panics() {
        for stream in [
            &[][..],
            &[0x8C, 0x06][..],
            &[0x8C, 0x06, 0xFF, 0xFF, 0xFF, 1, 2, 3][..],
        ] {
            assert!(decode(stream, 4096).is_err());
        }
    }

    #[test]
    fn a_stored_quantum_is_copied() {
        let mut stream = vec![0xCC, 0x06];
        stream.extend_from_slice(b"hello world");
        assert_eq!(decode(&stream, 11).unwrap(), b"hello world");
    }

    #[test]
    fn a_stream_of_two_stored_quanta_is_both_of_them() {
        let (a, b) = (vec![7u8; 0x40000], vec![9u8; 100]);
        let mut stream = vec![0xCC, 0x06];
        stream.extend_from_slice(&a);
        stream.extend_from_slice(&[0xCC, 0x06]);
        stream.extend_from_slice(&b);
        let got = decode(&stream, a.len() + b.len()).unwrap();
        assert_eq!((&got[..a.len()], &got[a.len()..]), (&a[..], &b[..]));
    }
}
