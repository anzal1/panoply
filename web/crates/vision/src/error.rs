//! Minimal stand-in for panoply_image::error, which pulls in C codecs that do not build for wasm32.
#[derive(Debug)]
pub enum ImageError {
    BadOption(String),
    Failed(String),
    Cancelled,
}
impl std::fmt::Display for ImageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImageError::BadOption(s) | ImageError::Failed(s) => write!(f, "{s}"),
            ImageError::Cancelled => write!(f, "Cancelled."),
        }
    }
}
impl std::error::Error for ImageError {}
pub type Result<T> = std::result::Result<T, ImageError>;
