//! Windows: the shell's `IFileOperation`, asked to recycle the item (`FOFX_RECYCLEONDELETE`), which is
//! what Explorer does for Delete. The item lands in the Recycle Bin and Explorer's Undo and Restore
//! work on it.
//!
//! Windows deletes for good, instead of recycling, when a drive has no Recycle Bin or a file is too big
//! for it. Two guards keep that from happening quietly. Before the call, only a fixed local drive is
//! accepted, so network shares, USB sticks and RAM disks are refused with an error and the file is
//! kept. During the call, `FOF_WANTNUKEWARNING` makes the shell ask the person first if it would
//! still have to delete for good (a too-big file, or a Recycle Bin that was switched off), and a "No"
//! comes back here as an error.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
use windows::Win32::Storage::FileSystem::{GetDriveTypeW, GetVolumePathNameW};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
    IBindCtx,
};
use windows::Win32::UI::Shell::{
    FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, FOF_WANTNUKEWARNING,
    FOFX_ADDUNDORECORD, FOFX_RECYCLEONDELETE, FileOperation, IFileOperation,
    IFileOperationProgressSink, IShellItem, SHCreateItemFromParsingName,
};
use windows::core::{IUnknown, PCWSTR};

use super::TrashError;

/// `DRIVE_FIXED` from `GetDriveTypeW`: a drive that is part of this computer.
const DRIVE_FIXED: u32 = 3;

pub(super) fn trash(path: &Path) -> Result<(), TrashError> {
    let abs = absolute(path)?;
    let wide = wide(abs.as_os_str());
    require_recycle_bin(&wide)?;

    // The shell wants a single-threaded COM apartment. A fresh thread has none yet, so it gets exactly
    // the one it needs whatever state the caller's thread is in.
    let job = std::thread::Builder::new()
        .name("panoply-recycle".into())
        .spawn(move || recycle(&wide))
        .map_err(TrashError::failed)?;
    job.join()
        .map_err(|_| TrashError::failed("the Windows shell call stopped unexpectedly"))??;

    // Trust the folder, not the return code.
    if abs.symlink_metadata().is_ok() {
        return Err(TrashError::failed("the item is still in its folder"));
    }
    Ok(())
}

/// An absolute path with forward slashes fixed and no `\\?\` prefix, which the shell does not take.
fn absolute(path: &Path) -> Result<PathBuf, TrashError> {
    let abs = std::path::absolute(path).map_err(TrashError::failed)?;
    let s = abs.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        return Ok(PathBuf::from(format!(r"\\{rest}")));
    }
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        return Ok(PathBuf::from(rest));
    }
    Ok(abs)
}

fn wide(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain(std::iter::once(0)).collect()
}

/// Only a fixed local drive has a Recycle Bin. Everything else is refused here, before anything
/// is touched.
#[allow(unsafe_code)]
fn require_recycle_bin(wide_path: &[u16]) -> Result<(), TrashError> {
    let mut root = [0u16; 1024];
    // SAFETY: `wide_path` is nul terminated and outlives the call; `root` is a buffer the API fills.
    let kind = unsafe {
        GetVolumePathNameW(PCWSTR(wide_path.as_ptr()), &mut root)
            .map_err(|e| TrashError::failed(e.message()))?;
        GetDriveTypeW(PCWSTR(root.as_ptr()))
    };
    if kind == DRIVE_FIXED {
        Ok(())
    } else {
        Err(TrashError::NoTrash)
    }
}

#[allow(unsafe_code)]
fn recycle(wide_path: &[u16]) -> Result<(), TrashError> {
    // SAFETY: CoInitializeEx and CoUninitialize are paired on this thread, and every COM object made
    // by `recycle_item` is dropped before CoUninitialize runs.
    unsafe {
        let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        // RPC_E_CHANGED_MODE cannot happen on a new thread, but COM is usable then too.
        let owns_apartment = hr.is_ok();
        if !owns_apartment && hr != RPC_E_CHANGED_MODE {
            return Err(TrashError::failed(windows::core::Error::from(hr).message()));
        }
        let r = recycle_item(wide_path);
        if owns_apartment {
            CoUninitialize();
        }
        r
    }
}

#[allow(unsafe_code)]
fn recycle_item(wide_path: &[u16]) -> Result<(), TrashError> {
    let why = |e: windows::core::Error| TrashError::failed(e.message());
    // SAFETY: `wide_path` is nul terminated and outlives the calls. The COM interfaces come from the
    // shell and are used from the thread that created them.
    unsafe {
        let item: IShellItem =
            SHCreateItemFromParsingName(PCWSTR(wide_path.as_ptr()), None::<&IBindCtx>)
                .map_err(why)?;
        let op: IFileOperation =
            CoCreateInstance(&FileOperation, None::<&IUnknown>, CLSCTX_ALL).map_err(why)?;
        op.SetOperationFlags(
            FOF_ALLOWUNDO
                | FOF_NOCONFIRMATION
                | FOF_WANTNUKEWARNING
                | FOF_NOERRORUI
                | FOF_SILENT
                | FOFX_ADDUNDORECORD
                | FOFX_RECYCLEONDELETE,
        )
        .map_err(why)?;
        op.DeleteItem(&item, None::<&IFileOperationProgressSink>)
            .map_err(why)?;
        op.PerformOperations().map_err(why)?;
        if op.GetAnyOperationsAborted().map_err(why)?.as_bool() {
            return Err(TrashError::failed("the delete was cancelled"));
        }
    }
    Ok(())
}

#[cfg(test)]
pub(super) mod tests {
    use super::super::{temp_dir, unique};
    use super::*;
    use std::fs;

    /// The current user's security identifier, which names their folder in `$Recycle.Bin`.
    fn current_sid() -> String {
        let out = std::process::Command::new("whoami")
            .args(["/user", "/fo", "csv", "/nh"])
            .output()
            .expect("whoami runs");
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        // "DOMAIN\user","S-1-5-21-..."
        text.trim()
            .rsplit(',')
            .next()
            .map(|s| s.trim_matches('"').to_string())
            .filter(|s| s.starts_with("S-1-"))
            .unwrap_or_else(|| panic!("no SID in `{text}`"))
    }

    /// The `$I` records in this user's bin on the drive of `original`, with their `$R` items.
    fn records(original: &Path) -> Vec<(PathBuf, PathBuf, String)> {
        let abs = absolute(original).unwrap();
        let drive = abs
            .components()
            .next()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .unwrap();
        let bin = PathBuf::from(format!(r"{drive}\$Recycle.Bin")).join(current_sid());
        let mut found = Vec::new();
        let Ok(rd) = fs::read_dir(&bin) else {
            return found;
        };
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            let Some(rest) = n.strip_prefix("$I") else {
                continue;
            };
            let Ok(bytes) = fs::read(e.path()) else {
                continue;
            };
            if let Some(orig) = original_path_in_record(&bytes) {
                found.push((e.path(), bin.join(format!("$R{rest}")), orig));
            }
        }
        found
    }

    /// The original path stored in a `$I` file. Version 1 (before Windows 10) has a fixed 520-byte
    /// path; version 2 has a length in characters, then the path.
    fn original_path_in_record(b: &[u8]) -> Option<String> {
        let version = u64::from_le_bytes(b.get(0..8)?.try_into().ok()?);
        let units: Vec<u16> = match version {
            1 => b.get(24..)?.chunks_exact(2),
            2 => {
                let len = u32::from_le_bytes(b.get(24..28)?.try_into().ok()?) as usize;
                b.get(28..28 + len * 2)?.chunks_exact(2)
            }
            _ => return None,
        }
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .take_while(|&u| u != 0)
        .collect();
        Some(String::from_utf16_lossy(&units))
    }

    /// The item in the Recycle Bin that came from a file with the same name as `original`. The tests
    /// use unique names, so the name is enough (the stored path can be the long form of a short temp
    /// path).
    pub(in super::super) fn find_in_recycle_bin(original: &Path) -> Option<PathBuf> {
        let want = original.file_name()?.to_string_lossy().to_lowercase();
        records(original)
            .into_iter()
            .find(|(_, _, orig)| orig.to_lowercase().ends_with(&want))
            .map(|(_, r, _)| r)
    }

    /// Take the test's item out of the bin again, so the tests leave the Recycle Bin as they found it.
    pub(in super::super) fn remove_from_recycle_bin(found: &Path) {
        let Some(rest) = found
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_prefix("$R"))
        else {
            return;
        };
        let _ = fs::remove_file(found).or_else(|_| fs::remove_dir_all(found));
        let _ = fs::remove_file(found.with_file_name(format!("$I{rest}")));
    }

    #[test]
    fn a_folder_goes_to_the_recycle_bin_with_what_is_in_it() {
        let d = temp_dir("panoply-win-folder");
        let name = unique("panoply-win-pages", "d");
        let folder = d.join(&name);
        fs::create_dir(&folder).unwrap();
        fs::write(folder.join("1.png"), "page").unwrap();

        super::super::move_to_trash(&folder).unwrap();

        assert!(!folder.exists());
        let found = find_in_recycle_bin(&folder).expect("the folder is in the Recycle Bin");
        assert_eq!(fs::read_to_string(found.join("1.png")).unwrap(), "page");
        remove_from_recycle_bin(&found);
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn forward_slashes_and_long_prefixes_are_accepted() {
        let d = temp_dir("panoply-win-slash");
        let name = unique("panoply-win-slash", "txt");
        let file = d.join(&name);
        fs::write(&file, "x").unwrap();
        let odd = PathBuf::from(file.to_string_lossy().replace('\\', "/"));

        super::super::move_to_trash(&odd).unwrap();

        assert!(!file.exists());
        let found = find_in_recycle_bin(&file).expect("the file is in the Recycle Bin");
        remove_from_recycle_bin(&found);
        fs::remove_dir_all(d).unwrap();
    }
}
