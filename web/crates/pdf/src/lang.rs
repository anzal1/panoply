//! Languages, scripts, and what each engine calls them.

use std::fmt;

use crate::error::OcrError;

/// The writing system of a language. It decides which recognizer reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Script {
    /// Latin, including accented letters.
    Latin,
    /// Devanagari: Hindi, Marathi, Nepali.
    Devanagari,
    /// Bengali.
    Bengali,
    /// Arabic.
    Arabic,
    /// Cyrillic.
    Cyrillic,
    /// Chinese, Japanese, Korean.
    Cjk,
    /// Thai.
    Thai,
}

/// One language Panoply can ask an engine to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Lang(&'static LangInfo);

/// The table row behind a [`Lang`].
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct LangInfo {
    /// Short code (`hi`).
    pub code: &'static str,
    /// English name.
    pub name: &'static str,
    /// Writing system.
    pub script: Script,
    /// Apple Vision's tag, when Vision reads it.
    pub vision: Option<&'static str>,
    /// BCP 47 tag for `Windows.Media.Ocr`.
    pub windows: &'static str,
    /// Tesseract language code.
    pub tesseract: &'static str,
}

macro_rules! langs {
    ($(($code:literal, $name:literal, $script:ident, $vision:expr, $win:literal, $tess:literal)),* $(,)?) => {
        const TABLE: &[LangInfo] = &[$(LangInfo {
            code: $code, name: $name, script: Script::$script, vision: $vision,
            windows: $win, tesseract: $tess,
        }),*];
    };
}

langs! {
    ("en", "English", Latin, Some("en-US"), "en-US", "eng"),
    ("fr", "French", Latin, Some("fr-FR"), "fr-FR", "fra"),
    ("de", "German", Latin, Some("de-DE"), "de-DE", "deu"),
    ("es", "Spanish", Latin, Some("es-ES"), "es-ES", "spa"),
    ("it", "Italian", Latin, Some("it-IT"), "it-IT", "ita"),
    ("pt", "Portuguese", Latin, Some("pt-BR"), "pt-BR", "por"),
    ("nl", "Dutch", Latin, Some("nl-NL"), "nl-NL", "nld"),
    ("sv", "Swedish", Latin, Some("sv-SE"), "sv-SE", "swe"),
    ("da", "Danish", Latin, Some("da-DK"), "da-DK", "dan"),
    ("nb", "Norwegian", Latin, Some("nb-NO"), "nb-NO", "nor"),
    ("pl", "Polish", Latin, Some("pl-PL"), "pl-PL", "pol"),
    ("cs", "Czech", Latin, Some("cs-CZ"), "cs-CZ", "ces"),
    ("ro", "Romanian", Latin, Some("ro-RO"), "ro-RO", "ron"),
    ("tr", "Turkish", Latin, Some("tr-TR"), "tr-TR", "tur"),
    ("id", "Indonesian", Latin, Some("id-ID"), "id-ID", "ind"),
    ("ms", "Malay", Latin, Some("ms-MY"), "ms-MY", "msa"),
    ("vi", "Vietnamese", Latin, Some("vi-VT"), "vi-VN", "vie"),
    ("hi", "Hindi", Devanagari, None, "hi-IN", "hin"),
    ("mr", "Marathi", Devanagari, None, "mr-IN", "mar"),
    ("ne", "Nepali", Devanagari, None, "ne-NP", "nep"),
    ("bn", "Bengali", Bengali, None, "bn-IN", "ben"),
    ("ar", "Arabic", Arabic, Some("ar-SA"), "ar-SA", "ara"),
    ("ru", "Russian", Cyrillic, Some("ru-RU"), "ru-RU", "rus"),
    ("uk", "Ukrainian", Cyrillic, Some("uk-UA"), "uk-UA", "ukr"),
    ("zh", "Chinese (Simplified)", Cjk, Some("zh-Hans"), "zh-Hans-CN", "chi_sim"),
    ("zh-tw", "Chinese (Traditional)", Cjk, Some("zh-Hant"), "zh-Hant-TW", "chi_tra"),
    ("ja", "Japanese", Cjk, Some("ja-JP"), "ja-JP", "jpn"),
    ("ko", "Korean", Cjk, Some("ko-KR"), "ko-KR", "kor"),
    ("th", "Thai", Thai, Some("th-TH"), "th-TH", "tha"),
}

impl Lang {
    /// English.
    pub const ENGLISH: Lang = Lang(&TABLE[0]);

    /// Every language in the table.
    pub fn all() -> impl Iterator<Item = Lang> {
        TABLE.iter().map(Lang)
    }

    /// Parse one language: a short code (`hi`), a code with a region (`en-US`), or the English
    /// name (`Hindi`), in any case.
    pub fn parse(s: &str) -> Result<Lang, OcrError> {
        let t = s.trim().to_lowercase().replace('_', "-");
        let found = TABLE
            .iter()
            .find(|l| l.code == t || l.name.to_lowercase() == t)
            .or_else(|| {
                let base = t.split('-').next().unwrap_or("");
                TABLE.iter().find(|l| {
                    l.code == base
                        // Region forms of the languages that have two entries.
                        && !(base == "zh" && (t.contains("tw") || t.contains("hant") || t.contains("hk")))
                })
            })
            .or_else(|| {
                (t.starts_with("zh") && (t.contains("tw") || t.contains("hant") || t.contains("hk")))
                    .then(|| TABLE.iter().find(|l| l.code == "zh-tw"))
                    .flatten()
            })
            .or_else(|| (t == "no").then(|| TABLE.iter().find(|l| l.code == "nb")).flatten());
        found.map(Lang).ok_or_else(|| {
            OcrError::Unsupported(format!(
                "\"{}\" is not a language Panoply can read yet. Try one of: {}.",
                s.trim(),
                Lang::all().map(|l| l.code()).collect::<Vec<_>>().join(", ")
            ))
        })
    }

    /// Parse a list such as `hi,en` or `Hindi, English`. Empty text means automatic (empty
    /// list). Repeats are dropped.
    pub fn parse_list(s: &str) -> Result<Vec<Lang>, OcrError> {
        let mut out: Vec<Lang> = Vec::new();
        for part in s.split([',', '+', ';']) {
            if part.trim().is_empty() || part.trim().eq_ignore_ascii_case("auto") {
                continue;
            }
            let l = Lang::parse(part)?;
            if !out.contains(&l) {
                out.push(l);
            }
        }
        Ok(out)
    }

    /// The short code.
    pub fn code(&self) -> &'static str {
        self.0.code
    }

    /// The English name.
    pub fn name(&self) -> &'static str {
        self.0.name
    }

    /// The writing system.
    pub fn script(&self) -> Script {
        self.0.script
    }

    /// Apple Vision's tag, when Vision reads this language.
    pub fn vision(&self) -> Option<&'static str> {
        self.0.vision
    }

    /// The BCP 47 tag `Windows.Media.Ocr` uses.
    pub fn windows(&self) -> &'static str {
        self.0.windows
    }

    /// The Tesseract language code.
    pub fn tesseract(&self) -> &'static str {
        self.0.tesseract
    }
}

impl fmt::Display for Lang {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0.name)
    }
}

/// A list of languages as one readable phrase: "Hindi and English".
pub fn phrase(langs: &[Lang]) -> String {
    match langs {
        [] => String::new(),
        [a] => a.name().to_string(),
        [a, b] => format!("{} and {}", a.name(), b.name()),
        [rest @ .., last] => format!(
            "{} and {}",
            rest.iter().map(|l| l.name()).collect::<Vec<_>>().join(", "),
            last.name()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_names_and_regions_parse() {
        assert_eq!(Lang::parse("hi").unwrap().code(), "hi");
        assert_eq!(Lang::parse("Hindi").unwrap().code(), "hi");
        assert_eq!(Lang::parse("en-US").unwrap().code(), "en");
        assert_eq!(Lang::parse("zh_Hant").unwrap().code(), "zh-tw");
        assert_eq!(Lang::parse("zh-Hans").unwrap().code(), "zh");
        assert_eq!(Lang::parse("no").unwrap().code(), "nb");
        assert!(Lang::parse("klingon").is_err());
    }

    #[test]
    fn lists_drop_repeats_and_auto() {
        let l = Lang::parse_list("hi, en,hi").unwrap();
        assert_eq!(l.len(), 2);
        assert!(Lang::parse_list("auto").unwrap().is_empty());
        assert!(Lang::parse_list("").unwrap().is_empty());
    }

    #[test]
    fn scripts_and_vision_coverage_match_the_measured_list() {
        assert_eq!(Lang::parse("hi").unwrap().script(), Script::Devanagari);
        assert_eq!(Lang::parse("bn").unwrap().script(), Script::Bengali);
        // Measured on macOS 26.6: Vision has neither.
        assert!(Lang::parse("hi").unwrap().vision().is_none());
        assert!(Lang::parse("bn").unwrap().vision().is_none());
        assert_eq!(Lang::parse("fr").unwrap().vision(), Some("fr-FR"));
    }

    #[test]
    fn phrases_read_naturally() {
        let l = Lang::parse_list("hi,en").unwrap();
        assert_eq!(phrase(&l), "Hindi and English");
        let l = Lang::parse_list("hi,en,bn").unwrap();
        assert_eq!(phrase(&l), "Hindi, English and Bengali");
    }
}
