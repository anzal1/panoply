//! Windows OCR is called from short lived threads (one per job, one per test). The runtime
//! objects it caches must stay valid after the thread that first made them has gone.

mod common;

#[test]
#[cfg(windows)]
fn windows_ocr_works_from_threads_that_come_and_go() {
    for _ in 0..4 {
        std::thread::spawn(|| {
            let _ = panoply_ocr::engine::windows_ocr::covers(&common::langs("en"));
        })
        .join()
        .expect("the thread did not crash");
    }
}
