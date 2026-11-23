//! A Bink movie's header, read by OpenRage; and, in tests on Windows, the `bink2w64.dll` a GTA V
//! install ships beside its executable, which decodes Bink 2 as the reference OpenRage's own
//! decoder is held to. The app loads no library: every platform decodes a Bink movie in `or-media`.

/// The fields at the head of every Bink 1 and Bink 2 file both installs ship.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub signature: [u8; 4],
    pub frames: u32,
    pub width: u32,
    pub height: u32,
    pub fps_num: u32,
    pub fps_den: u32,
    pub audio_tracks: u32,
}

/// Bytes 0..44 hold the same fields in `BIKi`, `KB2g` and `KB2j`; the audio
/// track tables after them moved by four bytes in `KB2j`.
const HEADER_LEN: usize = 44;

impl Header {
    /// The header of a file that starts `BIK` or `KB2`, else `None`.
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        if !(bytes.starts_with(b"BIK") || bytes.starts_with(b"KB2")) || bytes.len() < HEADER_LEN {
            return None;
        }
        let at = |offset: usize| {
            u32::from_le_bytes([
                bytes[offset],
                bytes[offset + 1],
                bytes[offset + 2],
                bytes[offset + 3],
            ])
        };
        Some(Self {
            signature: [bytes[0], bytes[1], bytes[2], bytes[3]],
            frames: at(8),
            width: at(20),
            height: at(24),
            fps_num: at(28),
            fps_den: at(32),
            audio_tracks: at(40),
        })
    }

    pub fn is_bink2(&self) -> bool {
        self.signature.starts_with(b"KB2")
    }

    /// The signature as text, `KB2g` or `BIKi`.
    pub fn revision(&self) -> String {
        String::from_utf8_lossy(&self.signature).into_owned()
    }

    /// Frames per second, or `None` when the file states a zero denominator.
    pub fn fps(&self) -> Option<f64> {
        (self.fps_den != 0).then(|| f64::from(self.fps_num) / f64::from(self.fps_den))
    }
}

/// The install's own decoder, loaded to hold OpenRage's to: never part of the app.
#[cfg(all(test, windows))]
pub mod oracle {
    use std::ffi::c_void;
    use std::marker::PhantomData;
    use std::os::windows::ffi::OsStrExt;
    use std::path::{Path, PathBuf};
    use std::ptr::NonNull;
    use std::sync::{Arc, Mutex, PoisonError};

    use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

    /// The library both GTA V installs ship beside the game executable.
    pub const DLL: &str = "bink2w64.dll";

    // Measured from both installs' DLLs: the export table, the argument registers each export
    // reads, and what an opened movie's handle and its frame buffers hold.
    type Open = unsafe extern "system" fn(*const c_void, u32) -> *mut u32;
    type SetSoundTrack = unsafe extern "system" fn(u32, *const u32);
    type Step = unsafe extern "system" fn(*mut u32) -> i32;
    type FrameBuffers = unsafe extern "system" fn(*mut u32, *mut u8);
    type CopyToBuffer =
        unsafe extern "system" fn(*mut u32, *mut u8, u32, u32, u32, u32, u32) -> i32;

    /// `BinkOpen`'s flag for a pointer to the file's bytes rather than to its path.
    const FROM_MEMORY: u32 = 0x0400_0000;
    /// `BinkOpen`'s flag to open only the sound tracks `BinkSetSoundTrack` named, here none.
    const SOUND_TRACKS: u32 = 0x4000;
    /// `BinkCopyToBuffer`'s surface of four bytes a pixel, which the DLL writes as B, G, R and one
    /// it leaves be.
    const SURFACE_BGRX: u32 = 3;
    /// `BinkCopyToBuffer`'s flag to copy the whole frame, not only what changed.
    const COPY_ALL: u32 = 0x8000_0000;

    // The handle's `u32` fields: width, height, frame count, and the frame rate as a numerator
    // and a denominator.
    const WIDTH: usize = 0;
    const HEIGHT: usize = 1;
    const FRAMES: usize = 2;
    const FPS_NUM: usize = 5;
    const FPS_DEN: usize = 6;

    /// The bytes of `BinkGetFrameBuffersInfo`'s answer that are read: the header is a count of
    /// buffers and the size of the luma and chroma planes, and the planes follow from byte 24, a
    /// set for each buffer.
    const BUFFERS_BYTES: usize = 512;
    const FIRST_SET: usize = 24;

    /// How a plane is laid out in the answer: the older DLL leaves the buffers to its caller, in
    /// planes of 24 bytes (an allocate flag, the buffer, its pitch) and four to a set, and the
    /// newer allocates them itself, in planes of 16 bytes (the buffer, a flag, the pitch) and five
    /// to a set.
    #[derive(Clone, Copy)]
    struct Layout {
        /// Whether the DLL allocates the planes, so that it names them again after each frame.
        owned: bool,
        plane: usize,
        set: usize,
        buffer: usize,
        pitch: usize,
    }

    const OLDER: Layout = Layout {
        owned: false,
        plane: 24,
        set: 96,
        buffer: 8,
        pitch: 16,
    };
    const NEWER: Layout = Layout {
        owned: true,
        plane: 16,
        set: 80,
        buffer: 0,
        pitch: 12,
    };

    /// The exports the oracle calls.
    pub struct Api {
        open: Open,
        set_sound_track: SetSoundTrack,
        do_frame: Step,
        next_frame: Step,
        close: Step,
        get_frame_buffers: FrameBuffers,
        register_frame_buffers: FrameBuffers,
        copy_to_buffer: CopyToBuffer,
    }

    // ⚠ Never unloaded: a movie may still be open in another thread. And opening is one at a time,
    // because naming the sound tracks to open is a setting of the library, not of a movie.
    static LOADED: Mutex<Vec<(PathBuf, &'static Api)>> = Mutex::new(Vec::new());
    static OPENING: Mutex<()> = Mutex::new(());

    /// The DLL at `path`, loaded once per process.
    pub fn load(path: &Path) -> Result<&'static Api, String> {
        let mut loaded = LOADED.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((_, api)) = loaded.iter().find(|(p, _)| p == path) {
            return Ok(api);
        }
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
        // SAFETY: a NUL-terminated path; a library the user's install ships.
        let module = unsafe { LoadLibraryW(wide.as_ptr()) };
        if module.is_null() {
            return Err(format!(
                "it did not load ({})",
                std::io::Error::last_os_error()
            ));
        }
        macro_rules! export {
            ($name:literal, $ty:ty) => {{
                // SAFETY: the export's signature was measured from the DLL; see the types above.
                match unsafe { GetProcAddress(module, concat!($name, "\0").as_ptr()) } {
                    Some(f) => unsafe {
                        std::mem::transmute::<unsafe extern "system" fn() -> isize, $ty>(f)
                    },
                    None => return Err(concat!("it exports no ", $name).to_string()),
                }
            }};
        }
        let api: &'static Api = Box::leak(Box::new(Api {
            open: export!("BinkOpen", Open),
            set_sound_track: export!("BinkSetSoundTrack", SetSoundTrack),
            do_frame: export!("BinkDoFrame", Step),
            next_frame: export!("BinkNextFrame", Step),
            close: export!("BinkClose", Step),
            get_frame_buffers: export!("BinkGetFrameBuffersInfo", FrameBuffers),
            register_frame_buffers: export!("BinkRegisterFrameBuffers", FrameBuffers),
            copy_to_buffer: export!("BinkCopyToBuffer", CopyToBuffer),
        }));
        loaded.push((path.to_path_buf(), api));
        Ok(api)
    }

    /// An opened movie, decoded a frame at a time into planes the test reads back.
    pub struct Movie {
        api: &'static Api,
        handle: NonNull<u32>,
        layout: Layout,
        /// What `BinkGetFrameBuffersInfo` last said, and what the older DLL was registered with: boxed,
        /// because the DLL keeps the address.
        buffers: Box<[u8; BUFFERS_BYTES]>,
        /// The frame the next `decode` is, counted from 0.
        frame: usize,
        // The DLL reads the movie from these bytes, and writes the planes the older one is given
        // into these buffers, until it is closed.
        _bytes: Arc<[u8]>,
        _planes: Vec<Vec<u8>>,
    }

    // SAFETY: the handle is used by one thread at a time, through `&mut self`.
    unsafe impl Send for Movie {}

    fn word(bytes: &[u8], at: usize) -> usize {
        u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]) as usize
    }

    impl Movie {
        /// Opens `bytes` with the DLL at `dll`, and says what it refused with if it did.
        pub fn open(dll: &Path, bytes: Arc<[u8]>) -> Result<Self, String> {
            let api = load(dll)?;
            let _opening = OPENING.lock().unwrap_or_else(PoisonError::into_inner);
            // SAFETY: zero tracks and no array; then a pointer to bytes the movie keeps alive.
            let handle = unsafe {
                (api.set_sound_track)(0, std::ptr::null());
                (api.open)(bytes.as_ptr().cast(), FROM_MEMORY | SOUND_TRACKS)
            };
            let Some(handle) = NonNull::new(handle) else {
                return Err("the DLL refused the movie".to_string());
            };
            let mut movie = Self {
                api,
                handle,
                layout: OLDER,
                buffers: Box::new([0; BUFFERS_BYTES]),
                frame: 0,
                _bytes: bytes,
                _planes: Vec::new(),
            };
            movie.read_buffers();
            // The newer DLL has already named its buffers, with the flag and a pitch where the older has
            // a pointer it left empty.
            if word(&movie.buffers[..], FIRST_SET + 8) == 1
                && word(&movie.buffers[..], FIRST_SET + 12) != 0
            {
                movie.layout = NEWER;
            } else {
                movie.register_planes();
            }
            Ok(movie)
        }

        fn read_buffers(&mut self) {
            // SAFETY: an open handle and a buffer larger than the answer.
            unsafe {
                (self.api.get_frame_buffers)(self.handle.as_ptr(), self.buffers.as_mut_ptr());
            }
        }

        /// Gives the older DLL the buffers it decodes into: a plane for each one it asks for.
        fn register_planes(&mut self) {
            let (total, luma, chroma) = (
                word(&self.buffers[..], 0),
                (word(&self.buffers[..], 4), word(&self.buffers[..], 8)),
                (word(&self.buffers[..], 12), word(&self.buffers[..], 16)),
            );
            let l = self.layout;
            for set in 0..total {
                for plane in 0..4 {
                    let at = FIRST_SET + set * l.set + plane * l.plane;
                    if word(&self.buffers[..], at) == 0 {
                        continue;
                    }
                    let (width, rows) = if plane == 0 || plane == 3 {
                        luma
                    } else {
                        chroma
                    };
                    let mut buffer = vec![0u8; width * rows + 64];
                    let address = buffer.as_mut_ptr() as u64;
                    self.buffers[at + l.buffer..at + l.buffer + 8]
                        .copy_from_slice(&address.to_le_bytes());
                    self.buffers[at + l.pitch..at + l.pitch + 4]
                        .copy_from_slice(&(width as u32).to_le_bytes());
                    self._planes.push(buffer);
                }
            }
            // SAFETY: an open handle; the buffers it names stay alive as long as it does.
            unsafe {
                (self.api.register_frame_buffers)(self.handle.as_ptr(), self.buffers.as_mut_ptr());
            }
        }

        fn field(&self, index: usize) -> u32 {
            // SAFETY: an open handle starts with these `u32` fields.
            unsafe { *self.handle.as_ptr().add(index) }
        }

        pub fn width(&self) -> u32 {
            self.field(WIDTH)
        }

        pub fn height(&self) -> u32 {
            self.field(HEIGHT)
        }

        pub fn frames(&self) -> u32 {
            self.field(FRAMES)
        }

        pub fn fps(&self) -> (u32, u32) {
            (self.field(FPS_NUM), self.field(FPS_DEN))
        }

        /// Decodes the next frame.
        pub fn decode(&mut self) {
            // SAFETY: an open handle.
            unsafe { (self.api.do_frame)(self.handle.as_ptr()) };
            if self.layout.owned {
                self.read_buffers();
            }
        }

        /// Steps past the frame decoded.
        pub fn next(&mut self) {
            // SAFETY: an open handle.
            unsafe { (self.api.next_frame)(self.handle.as_ptr()) };
            self.frame += 1;
        }

        /// A plane of the frame just decoded, 0 luma, 1 the red chroma and 2 the blue: the
        /// frame lies in the buffer its number picks.
        pub fn plane(&self, index: usize) -> Plane<'_> {
            let total = word(&self.buffers[..], 0).max(1);
            let at = FIRST_SET
                + ((self.frame + 1) % total) * self.layout.set
                + index * self.layout.plane;
            let buffer = u64::from_le_bytes(
                self.buffers[at + self.layout.buffer..at + self.layout.buffer + 8]
                    .try_into()
                    .unwrap_or([0; 8]),
            );
            Plane {
                buffer: buffer as *const u8,
                pitch: word(&self.buffers[..], at + self.layout.pitch),
                _movie: PhantomData,
            }
        }

        /// The frame just decoded as RGBA at the movie's size, the DLL's own conversion.
        pub fn rgba(&mut self) -> Vec<u8> {
            let (width, height) = (self.width(), self.height());
            let mut bgrx = vec![0u8; width as usize * height as usize * 4];
            // SAFETY: the buffer holds `height` rows of `width * 4` bytes.
            unsafe {
                (self.api.copy_to_buffer)(
                    self.handle.as_ptr(),
                    bgrx.as_mut_ptr(),
                    width * 4,
                    height,
                    0,
                    0,
                    SURFACE_BGRX | COPY_ALL,
                );
            }
            for pixel in bgrx.as_chunks_mut::<4>().0 {
                pixel.swap(0, 2);
                pixel[3] = 255;
            }
            bgrx
        }
    }

    impl Drop for Movie {
        fn drop(&mut self) {
            // SAFETY: closed once, here, before the bytes and the planes it reads go.
            unsafe { (self.api.close)(self.handle.as_ptr()) };
        }
    }

    /// One plane of the frame the DLL decoded last: rows of `pitch` bytes it owns.
    pub struct Plane<'a> {
        buffer: *const u8,
        pitch: usize,
        _movie: PhantomData<&'a Movie>,
    }

    impl Plane<'_> {
        /// The first `width` bytes of row `y`.
        pub fn row(&self, y: usize, width: usize) -> &[u8] {
            // SAFETY: the DLL decoded a plane of at least `y + 1` rows of `width` bytes at this
            // pitch, which the caller takes from the movie's own size.
            unsafe { std::slice::from_raw_parts(self.buffer.add(y * self.pitch), width) }
        }
    }
}

#[cfg(test)]
/// A `KB2g` header as both installs ship it, with made-up fields.
pub fn synthetic_bink2() -> Vec<u8> {
    let mut bytes = b"KB2g".to_vec();
    for field in [1000u32, 10, 100, 10, 640, 360, 2997, 100, 0x10, 1] {
        bytes.extend_from_slice(&field.to_le_bytes());
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bink2_header_states_its_kind_size_frames_rate_and_tracks() {
        let header = Header::parse(&synthetic_bink2()).expect("a Bink 2 header");
        assert!(header.is_bink2());
        assert_eq!(header.revision(), "KB2g");
        assert_eq!(
            (
                header.width,
                header.height,
                header.frames,
                header.audio_tracks
            ),
            (640, 360, 10, 1)
        );
        assert_eq!(header.fps(), Some(29.97));
    }

    #[test]
    fn a_bink1_header_is_not_bink2_and_other_bytes_are_no_header() {
        let mut bink1 = synthetic_bink2();
        bink1[..4].copy_from_slice(b"BIKi");
        assert!(!Header::parse(&bink1).expect("a Bink 1 header").is_bink2());
        assert_eq!(Header::parse(b"BIKi"), None, "a truncated header");
        assert_eq!(Header::parse(&[0u8; 64]), None);
    }
}
