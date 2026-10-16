//! What a result card shows besides the file names: which tools get the Before and After size
//! tiles, which output gets a thumbnail, and how big that thumbnail is drawn. No UI types, so the
//! decisions are unit tested; `ui/panel.rs` only draws what this says.

use std::path::{Path, PathBuf};

use crate::jobs::Done;

/// Longest side, in pixels, of the preview decoded for the card. About twice the largest size it is
/// drawn at, so it stays sharp on a 2x screen without holding a whole photo in memory.
pub const PREVIEW_SIDE: u32 = 960;
/// The largest a thumbnail is drawn, in points.
pub const THUMB_MAX: (f32, f32) = (440.0, 224.0);

/// Tools whose point is a smaller or different file, so the sizes are the answer. Everything else
/// (a print sheet, a cutout, an erased area, a passport crop, normalised sound, stripped metadata)
/// makes a file whose size is incidental, and "209% bigger" is noise or an alarm there.
const SIZE_IS_THE_GOAL: [&str; 7] = [
    "image.compress",
    "image.resize_to_kb",
    "image.resize",
    "image.convert",
    "media.compress",
    "media.convert",
    "pdf.compress",
];

/// True when the card shows Before and After size tiles for this tool: it is on the list above, or
/// it has a target size option of its own (a person who types a size cares about the size).
pub fn size_is_the_goal(tool_id: &str, has_size_option: bool) -> bool {
    has_size_option || SIZE_IS_THE_GOAL.contains(&tool_id)
}

/// A tool that works on pictures, so the card can compare the first input with the first output.
pub fn is_picture_tool(tool_id: &str) -> bool {
    tool_id.starts_with("image.") || tool_id.starts_with("id.")
}

/// A file Panoply can make a preview of, judged by its name so a video or a PDF is never opened.
pub fn is_picture_file(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "jpg"
            | "jpeg"
            | "png"
            | "webp"
            | "gif"
            | "bmp"
            | "tif"
            | "tiff"
            | "heic"
            | "heif"
            | "avif"
            | "jxl"
            | "jp2"
    )
}

/// The file the card shows a thumbnail of: the first output, when it is a picture.
pub fn thumb_source(outputs: &[PathBuf]) -> Option<&Path> {
    outputs
        .first()
        .map(PathBuf::as_path)
        .filter(|p| is_picture_file(p))
}

/// The picture to compare against: the first input, for the picture tools only. A tool that turns
/// a PDF into pictures has nothing to compare, and the thumbnail alone is shown.
pub fn compare_source<'a>(tool_id: &str, inputs: &'a [PathBuf]) -> Option<&'a Path> {
    if !is_picture_tool(tool_id) {
        return None;
    }
    inputs
        .first()
        .map(PathBuf::as_path)
        .filter(|p| is_picture_file(p))
}

/// The size a picture of `w` by `h` pixels is drawn at, in points, inside `max`: the largest that
/// fits, never more than one point per pixel (a tiny picture is not blown up into mush), and never
/// zero in either direction.
pub fn fit_within(w: u32, h: u32, max: (f32, f32)) -> (f32, f32) {
    if w == 0 || h == 0 {
        return (0.0, 0.0);
    }
    let (wf, hf) = (w as f32, h as f32);
    let scale = (max.0 / wf).min(max.1 / hf).min(1.0);
    ((wf * scale).round().max(1.0), (hf * scale).round().max(1.0))
}

/// Straight RGBA to the premultiplied BGRA that GPUI draws, and whether any pixel is see-through.
pub fn to_bgra_premultiplied(rgba: &[u8]) -> (Vec<u8>, bool) {
    let mut out = Vec::with_capacity(rgba.len());
    let mut alpha = false;
    for &[r, g, b, a8] in rgba.as_chunks::<4>().0 {
        let a = a8 as u32;
        alpha |= a != 255;
        let mul = |c: u8| ((c as u32 * a + 127) / 255) as u8;
        out.extend_from_slice(&[mul(b), mul(g), mul(r), a8]);
    }
    (out, alpha)
}

/// The title of a result card. Where size is the goal it is the usual "4.2 MB to 880 KB, 79%
/// smaller". Elsewhere a percentage of growth is not news, so a job that made one file per input
/// is just "Done" (or "Done, 3 files"); a title that is not about sizes (a text answer, "6 files,
/// 12 MB") is kept.
pub fn card_title(done: &Done, inputs: usize, size_matters: bool) -> String {
    let headline = done.headline(inputs);
    let one_per_input = done.after_bytes.is_some()
        && done.before_bytes > 0
        && inputs > 0
        && done.outputs.len() == inputs;
    if size_matters || !one_per_input {
        return headline;
    }
    match done.outputs.len() {
        1 => "Done".to_string(),
        n => format!("Done, {n} files"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn done(before: u64, after: u64, outputs: usize) -> Done {
        Done {
            outputs: (0..outputs)
                .map(|i| PathBuf::from(format!("/a/{i}.png")))
                .collect(),
            notes: vec!["a note".into()],
            before_bytes: before,
            after_bytes: Some(after),
        }
    }

    #[test]
    fn size_tiles_stay_only_where_size_is_the_goal() {
        for id in [
            "image.compress",
            "image.resize_to_kb",
            "image.convert",
            "image.resize",
            "media.compress",
            "media.convert",
            "pdf.compress",
        ] {
            assert!(size_is_the_goal(id, false), "{id} keeps its size tiles");
        }
        for id in [
            "id.print_sheet",
            "id.passport_photo",
            "id.signature_cleaner",
            "image.remove_background",
            "image.erase",
            "image.remove_watermark",
            "image.upscale",
            "image.strip_metadata",
            "media.normalize",
            "media.trim",
            "pdf.to_images",
            "pdf.merge",
        ] {
            assert!(!size_is_the_goal(id, false), "{id} shows no size tiles");
        }
        // A target size option makes the size the goal for a tool that is not on the list.
        assert!(size_is_the_goal("community.shrinker", true));
        assert!(!size_is_the_goal("community.shrinker", false));
    }

    #[test]
    fn a_title_never_says_percent_bigger_where_size_is_not_the_goal() {
        let grown = done(1_200_000, 3_700_000, 1);
        assert!(card_title(&grown, 1, true).contains("% bigger"));
        assert_eq!(card_title(&grown, 1, false), "Done");
        let three = done(1_200_000, 3_700_000, 3);
        assert_eq!(card_title(&three, 3, false), "Done, 3 files");
        // Not one file per input ("2 files, 3.7 MB") and text answers are untouched.
        let sheet = done(1_200_000, 3_700_000, 2);
        assert_eq!(card_title(&sheet, 1, false), sheet.headline(1));
        let text = Done {
            outputs: vec![],
            notes: vec!["Hello".into()],
            before_bytes: 10,
            after_bytes: None,
        };
        assert_eq!(card_title(&text, 1, false), "Hello");
    }

    #[test]
    fn the_thumbnail_is_the_first_output_and_only_when_it_is_a_picture() {
        let pics = vec![
            PathBuf::from("/a/page-1.PNG"),
            PathBuf::from("/a/page-2.png"),
        ];
        assert_eq!(thumb_source(&pics), Some(Path::new("/a/page-1.PNG")));
        let video = vec![PathBuf::from("/a/clip.mp4"), PathBuf::from("/a/x.png")];
        assert_eq!(thumb_source(&video), None, "the first output decides");
        for name in ["a.pdf", "a.wav", "a.mkv", "a", "a.svg"] {
            assert!(
                !is_picture_file(Path::new(name)),
                "{name} gets no thumbnail"
            );
        }
        assert_eq!(thumb_source(&[]), None);
    }

    #[test]
    fn only_picture_tools_compare_with_their_first_input() {
        let inputs = vec![PathBuf::from("/a/in.jpg"), PathBuf::from("/a/in2.jpg")];
        for id in [
            "image.remove_background",
            "id.passport_photo",
            "image.compress",
        ] {
            assert_eq!(compare_source(id, &inputs), Some(Path::new("/a/in.jpg")));
        }
        // PDF to images shows page one alone: it is not a picture tool, and a PDF is not a picture.
        let pdf = vec![PathBuf::from("/a/in.pdf")];
        assert_eq!(compare_source("pdf.to_images", &pdf), None);
        assert_eq!(compare_source("image.convert", &pdf), None);
    }

    #[test]
    fn a_thumbnail_fits_its_box_and_is_never_blown_up() {
        assert_eq!(
            fit_within(4000, 3000, THUMB_MAX),
            (299.0, 224.0),
            "limited by the height"
        );
        assert_eq!(
            fit_within(3000, 500, THUMB_MAX),
            (440.0, 73.0),
            "limited by the width"
        );
        let (w, h) = fit_within(413, 531, THUMB_MAX);
        assert_eq!(h, 224.0);
        assert!((w - 174.0).abs() <= 1.0);
        assert_eq!(
            fit_within(64, 48, THUMB_MAX),
            (64.0, 48.0),
            "one point per pixel at most"
        );
        assert_eq!(fit_within(0, 10, THUMB_MAX), (0.0, 0.0));
        let (w, h) = fit_within(10_000, 1, THUMB_MAX);
        assert!(w <= 440.0 && h >= 1.0, "a hairline picture keeps one point");
    }

    #[test]
    fn pixels_are_premultiplied_and_alpha_is_noticed() {
        let (out, alpha) = to_bgra_premultiplied(&[200, 100, 50, 255, 200, 100, 50, 128]);
        assert!(alpha);
        assert_eq!(&out[..4], &[50, 100, 200, 255], "opaque pixels only swap");
        assert_eq!(&out[4..], &[25, 50, 100, 128], "see-through pixels scale");
        let (_, alpha) = to_bgra_premultiplied(&[1, 2, 3, 255]);
        assert!(!alpha, "a fully opaque picture needs no checker");
    }
}
