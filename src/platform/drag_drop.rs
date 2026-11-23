//! The two halves of dragging files across the window's edge that winit does
//! not provide: where the cursor is during a drop, and starting a drag out.

use std::path::PathBuf;

use winit::window::Window;

/// The cursor in the window's client area, in physical pixels.
///
/// ⚠ **winit reports a dropped file without a position**, and Explorer's drag
/// loop sends the target window no mouse moves, so egui's last pointer
/// position is wherever the cursor was before the drag came in. Asking the OS
/// at the moment of the drop is what lets a drop aim at a row.
#[cfg(windows)]
pub fn cursor_in_client(window: &Window) -> Option<(f64, f64)> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::Graphics::Gdi::ScreenToClient;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;

    let hwnd = hwnd_of(window)?;
    let mut p = POINT { x: 0, y: 0 };
    // SAFETY: both calls write only into `p`, which outlives them, and
    // `hwnd` is the live window winit handed us.
    let ok = unsafe { GetCursorPos(&mut p) != 0 && ScreenToClient(hwnd, &mut p) != 0 };
    ok.then_some((p.x as f64, p.y as f64))
}

#[cfg(not(windows))]
pub fn cursor_in_client(_window: &Window) -> Option<(f64, f64)> {
    None
}

#[cfg(windows)]
fn hwnd_of(window: &Window) -> Option<windows_sys::Win32::Foundation::HWND> {
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match window.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(h) => Some(h.hwnd.get() as windows_sys::Win32::Foundation::HWND),
        _ => None,
    }
}

/// Hand files to the shell as a drag, and return once they are dropped or
/// the drag is cancelled.
///
/// ⚠ **Modal, and it has to be.** The shell's drag loop owns the mouse until
/// the button comes up, so it runs after the frame that asked for it rather
/// than inside it. Every path must sit in one directory, which is how the
/// staging lays them out.
#[cfg(windows)]
pub fn drag_files_out(window: &Window, paths: &[PathBuf]) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::core::GUID;
    use windows_sys::Win32::System::Ole::DROPEFFECT_COPY;
    use windows_sys::Win32::UI::Shell::Common::ITEMIDLIST;
    use windows_sys::Win32::UI::Shell::{
        ILFindLastID, ILFree, SHCreateDataObject, SHDoDragDrop, SHParseDisplayName,
    };

    const IID_IDATAOBJECT: GUID = GUID::from_u128(0x0000010e_0000_0000_c000_000000000046);

    #[repr(C)]
    struct UnknownVtbl {
        query_interface: usize,
        add_ref: usize,
        release: unsafe extern "system" fn(*mut std::ffi::c_void) -> u32,
    }

    let hwnd = hwnd_of(window).ok_or("the window has no Win32 handle")?;
    let parent = paths
        .first()
        .and_then(|p| p.parent())
        .ok_or("nothing to drag")?
        .to_path_buf();
    if paths.iter().any(|p| p.parent() != Some(parent.as_path())) {
        return Err("dragged files must share one folder".into());
    }
    let wide = |p: &std::path::Path| -> Vec<u16> {
        p.as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    };
    let parse = |p: &std::path::Path| -> Result<*mut ITEMIDLIST, String> {
        let name = wide(p);
        let mut pidl: *mut ITEMIDLIST = std::ptr::null_mut();
        // SAFETY: `name` is NUL-terminated and lives across the call; the
        // shell allocates `pidl`, which the caller frees with ILFree.
        let hr = unsafe {
            SHParseDisplayName(
                name.as_ptr(),
                std::ptr::null_mut(),
                &mut pidl,
                0,
                std::ptr::null_mut(),
            )
        };
        if hr < 0 || pidl.is_null() {
            Err(format!(
                "{}: the shell could not name it ({hr:#x})",
                p.display()
            ))
        } else {
            Ok(pidl)
        }
    };

    let folder = parse(&parent)?;
    let mut items: Vec<*mut ITEMIDLIST> = Vec::new();
    let result = (|| -> Result<(), String> {
        for p in paths {
            items.push(parse(p)?);
        }
        // SAFETY: each child pointer is the last element of an absolute
        // pidl we own, so it is relative to `folder` and outlives the call.
        let children: Vec<*const ITEMIDLIST> = items
            .iter()
            .map(|&abs| unsafe { ILFindLastID(abs) as *const ITEMIDLIST })
            .collect();
        let mut data: *mut std::ffi::c_void = std::ptr::null_mut();
        // SAFETY: arguments are as SHCreateDataObject documents them, and
        // `data` receives an owned IDataObject reference.
        let hr = unsafe {
            SHCreateDataObject(
                folder,
                children.len() as u32,
                children.as_ptr(),
                std::ptr::null_mut(),
                &IID_IDATAOBJECT,
                &mut data,
            )
        };
        if hr < 0 || data.is_null() {
            return Err(format!("the shell would not describe the files ({hr:#x})"));
        }
        let mut effect = 0u32;
        // SAFETY: `data` is a live IDataObject; a null drop source asks the
        // shell for its default one. The reference is released once, below.
        let hr = unsafe {
            let hr = SHDoDragDrop(
                hwnd,
                data,
                std::ptr::null_mut(),
                DROPEFFECT_COPY,
                &mut effect,
            );
            let vtbl = *(data as *const *const UnknownVtbl);
            ((*vtbl).release)(data);
            hr
        };
        if hr < 0 {
            return Err(format!("the drag failed ({hr:#x})"));
        }
        Ok(())
    })();
    // SAFETY: every pidl here came from SHParseDisplayName and is freed once.
    unsafe {
        for pidl in items {
            ILFree(pidl);
        }
        ILFree(folder);
    }
    result
}

#[cfg(not(windows))]
pub fn drag_files_out(_window: &Window, _paths: &[PathBuf]) -> Result<(), String> {
    Err("dragging files out of the window is Windows-only".into())
}
