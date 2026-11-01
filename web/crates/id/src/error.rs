#[derive(Debug)]
pub enum ImageError {
    Failed(String),
    BadOption(String),
}
impl std::fmt::Display for ImageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImageError::Failed(s) | ImageError::BadOption(s) => write!(f, "{s}"),
        }
    }
}
impl std::error::Error for ImageError {}
pub type Result<T> = std::result::Result<T, ImageError>;
