//! `panoply-desktop`: the Panoply app for macOS, Windows and Linux.

fn main() {
    panoply_desktop::run_from_args(std::env::args().skip(1).collect());
}
