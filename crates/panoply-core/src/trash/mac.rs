//! macOS: `NSFileManager trashItemAtURL`, the call Finder's Move to Trash uses. The item can be put
//! back from the Trash, and a file on another volume goes to that volume's own `.Trashes`.

use std::path::Path;

use objc2_foundation::{NSFileManager, NSString, NSURL};

use super::TrashError;

pub(super) fn trash(path: &Path) -> Result<(), TrashError> {
    let text = path
        .to_str()
        .ok_or_else(|| TrashError::failed("the path is not valid text"))?;
    let url = NSURL::fileURLWithPath(&NSString::from_str(text));
    let manager = NSFileManager::defaultManager();
    manager
        .trashItemAtURL_resultingItemURL_error(&url, None)
        .map_err(|e| TrashError::failed(e.localizedDescription()))
}
