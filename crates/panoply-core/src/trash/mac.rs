//! macOS: `NSFileManager trashItemAtURL`, the call Finder's Move to Trash uses. The item can be put
//! back from the Trash, and a file on another volume goes to that volume's own `.Trashes`.

use std::path::{Path, PathBuf};

use objc2_foundation::{NSFileManager, NSString, NSURL};

use super::TrashError;

/// Trash `path` and return where the system put it (the `resultingItemURL`), when it says.
pub(super) fn trash(path: &Path) -> Result<Option<PathBuf>, TrashError> {
    let text = path
        .to_str()
        .ok_or_else(|| TrashError::failed("the path is not valid text"))?;
    let url = NSURL::fileURLWithPath(&NSString::from_str(text));
    let manager = NSFileManager::defaultManager();
    let mut resulting = None;
    manager
        .trashItemAtURL_resultingItemURL_error(&url, Some(&mut resulting))
        .map_err(|e| TrashError::failed(e.localizedDescription()))?;
    Ok(resulting
        .and_then(|u| u.path())
        .map(|p| PathBuf::from(p.to_string())))
}
