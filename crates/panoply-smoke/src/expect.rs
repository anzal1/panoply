//! The refusals the matrix expects.
//!
//! A module may decline a file, and when it does it must say so in plain words with a next step
//! (the child already checks that). That is not enough to call the run clean: a module that
//! declines a file it should read is a bug that wears a polite message. So a decline counts as
//! clean only when one of the rules below explains it, from what is known about the input: the
//! corpus' tags, and whether the file has sound. A decline no rule explains is a failure.
//!
//! Each rule names the sentence the module says (a part of it), who may say it, and when. Add a
//! rule only when the refusal is right for that kind of input, never to quiet a run.

use crate::matrix::Row;

/// What a rule looks at.
pub struct Subject<'a> {
    /// Module id.
    pub module: &'a str,
    /// The case group: the file type, `defaults only`, `batch of N type`.
    pub group: &'a str,
    /// The traits of each input.
    pub traits: &'a [Vec<String>],
    /// The refusal sentence.
    pub says: &'a str,
}

impl Subject<'_> {
    /// True when at least one input has the trait.
    pub fn any(&self, t: &str) -> bool {
        self.traits.iter().any(|f| f.iter().any(|x| x == t))
    }

    /// True when every input has the trait.
    pub fn all(&self, t: &str) -> bool {
        !self.traits.is_empty() && self.traits.iter().all(|f| f.iter().any(|x| x == t))
    }

    /// The file type of the case, empty for `defaults only` and batches.
    pub fn ext(&self) -> &str {
        self.group.split(' ').next().unwrap_or("")
    }
}

/// One expected refusal.
pub struct Rule {
    /// Short name, shown in the report.
    pub id: &'static str,
    /// A part of the sentence the module says.
    pub says: &'static str,
    /// Why saying no is right.
    pub why: &'static str,
    /// When it applies.
    pub when: fn(&Subject) -> bool,
}

/// Every refusal that is right for the input it was given.
pub const RULES: &[Rule] = &[
    Rule {
        id: "needs a choice",
        says: "",
        why: "The case runs the module with its default options on purpose. These modules have no default for the answer that is the person's (which country, how big, what to erase or keep, which pages), so they refuse in plain words and say what to type.",
        when: |s| s.group == "defaults only",
    },
    Rule {
        id: "animated, asks first",
        says: "is animated",
        why: "Panoply does not write animated files yet, so by default it asks before dropping frames. The same file runs with animation=first in its own case.",
        when: |s| s.any("animated"),
    },
    Rule {
        id: "arithmetic-coded JPEG",
        says: "arithmetic coding",
        why: "A rare JPEG variant the decoder cannot read yet. Panoply says so and asks for a standard JPEG or PNG (panoply-image README).",
        when: |s| s.any("arithmetic-coding"),
    },
    Rule {
        id: "subsampled YCbCr TIFF",
        says: "YCbCr",
        why: "A TIFF flavor the decoder cannot read yet. Panoply says so and asks for a plain TIFF or PNG (panoply-image README).",
        when: |s| s.any("ycbcr"),
    },
    Rule {
        id: "system decoder missing",
        says: "",
        why: "HEIC, AVIF, JPEG XL, JPEG 2000, OpenEXR and camera RAW read through the system's decoder, which is wired on macOS only. Elsewhere Panoply says so in a plain sentence and what to do meanwhile (docs/KNOWN-LIMITATIONS.md, differences between systems).",
        when: |s| {
            !cfg!(target_os = "macos")
                && (s.says.contains("HEIC helper") || s.says.contains("operating system's decoder"))
        },
    },
    Rule {
        id: "no face in the picture",
        says: "No face was found",
        why: "The photo has no face to build a passport photo around. A face photo in the same run must not be refused.",
        when: |s| !s.all("face"),
    },
    Rule {
        id: "no ink in the picture",
        says: "No ink was found",
        why: "The picture holds no signature, so there is nothing to clean.",
        when: |s| s.module == "id.signature_cleaner" && !s.all("signature"),
    },
    Rule {
        id: "no watermark found",
        says: "No watermark",
        why: "The corpus has no watermarked picture. Panoply says nothing was changed and how to mark the area by hand.",
        when: |s| s.module == "image.remove_watermark" && !s.any("watermark"),
    },
    Rule {
        id: "no sound in the file",
        says: "no sound to work with",
        why: "The file has no audio stream (checked with ffprobe).",
        when: |s| s.any("no-audio"),
    },
    Rule {
        id: "no metadata container",
        says: "not supported for metadata work",
        why: "GIF, ICO, JPEG 2000 and EXR have no EXIF or XMP container to strip.",
        when: |s| {
            s.module == "image.strip_metadata" && matches!(s.ext(), "gif" | "ico" | "jp2" | "exr")
        },
    },
    Rule {
        id: "no text layer to read aloud",
        says: "no text layer to read",
        why: "Text to speech reads a PDF from its own text layer. A scan or a blank page has none, and Panoply says so and to run PDF OCR first. In a batch the file is skipped and the others are read.",
        when: |s| s.module == "text.to_speech",
    },
    Rule {
        id: "too much text for a QR code",
        says: "too much text for one QR code",
        why: "A QR code holds about 2,900 characters at most. A line longer than that (an SVG or a document handed over as text) cannot be one code, and the message names the line and says to shorten it.",
        when: |s| s.module == "image.qr_code",
    },
    Rule {
        id: "too large or too small to upscale",
        says: "",
        why: "Upscaling a picture that is already large would not help and would not fit the memory limit, and upscaling one that is already sharp gains little.",
        when: |s| {
            s.module == "image.upscale"
                && (s.says.contains("already large") || s.says.contains("gains little"))
        },
    },
];

/// The rule that explains a refusal, if any.
pub fn explain(row: &Row) -> Option<&'static Rule> {
    let says = row.result.as_ref()?.error.as_deref()?;
    let subject = Subject {
        module: &row.module,
        group: &row.group,
        traits: &row.traits,
        says,
    };
    RULES
        .iter()
        .find(|r| (r.says.is_empty() || says.contains(r.says)) && (r.when)(&subject))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn subject<'a>(
        module: &'a str,
        group: &'a str,
        traits: &'a [Vec<String>],
        says: &'a str,
    ) -> Subject<'a> {
        Subject {
            module,
            group,
            traits,
            says,
        }
    }

    fn t(tags: &[&str]) -> Vec<Vec<String>> {
        vec![tags.iter().map(|s| s.to_string()).collect()]
    }

    fn rule(id: &str) -> &'static Rule {
        RULES.iter().find(|r| r.id == id).unwrap()
    }

    #[test]
    fn a_face_photo_may_not_be_refused_for_having_no_face() {
        let face = t(&["face", "jpeg"]);
        let none = t(&["jpeg"]);
        let r = rule("no face in the picture");
        assert!(!(r.when)(&subject(
            "id.passport_photo",
            "jpg",
            &face,
            "No face was found"
        )));
        assert!((r.when)(&subject(
            "id.passport_photo",
            "jpg",
            &none,
            "No face was found"
        )));
    }

    #[test]
    fn a_defaults_only_case_may_refuse_but_a_normal_case_may_not() {
        let r = rule("needs a choice");
        assert!((r.when)(&subject(
            "image.erase",
            "defaults only",
            &[],
            "Say what to erase"
        )));
        assert!(!(r.when)(&subject(
            "image.erase",
            "jpg",
            &[],
            "Say what to erase"
        )));
    }

    #[test]
    fn a_silent_file_is_the_only_reason_to_refuse_for_missing_sound() {
        let r = rule("no sound in the file");
        assert!((r.when)(&subject(
            "media.normalize",
            "mp4",
            &t(&["no-audio"]),
            ""
        )));
        assert!(!(r.when)(&subject(
            "media.normalize",
            "mp4",
            &t(&["h264"]),
            ""
        )));
    }
}
