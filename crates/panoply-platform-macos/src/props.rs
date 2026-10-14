use objc2_core_foundation::{CFNumber, CFRetained, CFString, CFType};

use crate::cf::{self, PropDict, sys};
use objc2_image_io::kCGImagePropertyOrientation;

/// An opaque copy of an image's property dictionaries (EXIF, TIFF, GPS, IPTC and so on).
#[derive(Clone)]
pub struct Properties {
    dict: Option<CFRetained<PropDict>>,
}

// SAFETY: the wrapped CFDictionary is never mutated after it is built, and CoreFoundation
// collections that are not mutated are safe to read and retain or release from any thread.
unsafe impl Send for Properties {}
// SAFETY: see the `Send` impl. Shared access only reads an immutable dictionary.
unsafe impl Sync for Properties {}

impl std::fmt::Debug for Properties {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Properties")
            .field("sections", &self.section_names())
            .finish()
    }
}

/// Sections worth carrying from a source file to a re-encoded one.
const KEPT_SECTIONS: [&str; 5] = ["{Exif}", "{ExifAux}", "{TIFF}", "{IPTC}", "{GPS}"];

impl Properties {
    pub(crate) fn from_dict(dict: Option<CFRetained<PropDict>>) -> Self {
        Properties { dict }
    }

    /// An empty set of properties.
    pub fn empty() -> Self {
        Properties { dict: None }
    }

    /// Builds properties from named sections of string or integer values. Intended for tests
    /// and for callers that want to write their own EXIF, for example
    /// `("{Exif}", &[("UserComment", "x")])`.
    pub fn from_sections(sections: &[(&str, &[(&str, PropValue)])]) -> Self {
        let mut keys: Vec<CFRetained<CFString>> = Vec::new();
        let mut values: Vec<CFRetained<CFType>> = Vec::new();
        for (section, entries) in sections {
            let k: Vec<CFRetained<CFString>> = entries
                .iter()
                .map(|(name, _)| CFString::from_str(name))
                .collect();
            let v: Vec<CFRetained<CFType>> = entries
                .iter()
                .map(|(_, value)| match value {
                    PropValue::Str(s) => CFString::from_str(s).into_cf_type(),
                    PropValue::Int(n) => CFNumber::new_i64(*n).into_cf_type(),
                    PropValue::Float(x) => CFNumber::new_f64(*x).into_cf_type(),
                })
                .collect();
            let kr: Vec<&CFString> = k.iter().map(|x| &**x).collect();
            let vr: Vec<&CFType> = v.iter().map(|x| &**x).collect();
            let inner = PropDict::from_slices(&kr, &vr);
            keys.push(CFString::from_str(section));
            values.push(inner.into_cf_type());
        }
        let kr: Vec<&CFString> = keys.iter().map(|x| &**x).collect();
        let vr: Vec<&CFType> = values.iter().map(|x| &**x).collect();
        Properties {
            dict: Some(PropDict::from_slices(&kr, &vr)),
        }
    }

    /// True when there is nothing in it.
    pub fn is_empty(&self) -> bool {
        self.dict.as_ref().is_none_or(|d| d.is_empty())
    }

    /// Names of the top level entries, for example `{Exif}`, `{TIFF}`, `{GPS}`.
    pub fn section_names(&self) -> Vec<String> {
        match &self.dict {
            Some(d) => d.to_vecs().0.iter().map(|k| k.to_string()).collect(),
            None => Vec::new(),
        }
    }

    /// True when the named section (for example `{GPS}`) is present and not empty.
    pub fn has_section(&self, section: &str) -> bool {
        let Some(d) = &self.dict else { return false };
        match cf::get_dict(d, &CFString::from_str(section)) {
            Some(inner) => !inner.is_empty(),
            None => false,
        }
    }

    /// The value of `key` inside `section` as text, numbers included.
    pub fn section_value(&self, section: &str, key: &str) -> Option<String> {
        let d = self.dict.as_ref()?;
        let inner = cf::get_dict(d, &CFString::from_str(section))?;
        cf::get_string(&inner, &CFString::from_str(key))
    }

    /// A top level number or string entry as text (for example `Headroom`, `Depth`).
    pub fn top_value(&self, key: &str) -> Option<String> {
        cf::get_string(self.dict.as_ref()?, &CFString::from_str(key))
    }

    /// The top level orientation value, if any.
    pub fn orientation(&self) -> Option<u8> {
        let d = self.dict.as_ref()?;
        cf::get_i64(d, sys!(kCGImagePropertyOrientation)).and_then(|v| u8::try_from(v).ok())
    }

    /// The dictionary to hand to ImageIO when encoding: only the metadata sections, with
    /// orientation forced to 1 and the pixel dimensions dropped.
    pub(crate) fn for_output(
        &self,
        drop_gps: bool,
    ) -> Vec<(CFRetained<CFString>, CFRetained<CFType>)> {
        let mut out: Vec<(CFRetained<CFString>, CFRetained<CFType>)> = Vec::new();
        let Some(dict) = &self.dict else { return out };
        let (keys, values) = dict.to_vecs();
        for (key, value) in keys.into_iter().zip(values) {
            let name = key.to_string();
            let keep =
                KEPT_SECTIONS.contains(&name.as_str()) || name == "DPIWidth" || name == "DPIHeight";
            if !keep || (drop_gps && name == "{GPS}") {
                continue;
            }
            let value = match value.downcast::<objc2_core_foundation::CFDictionary>() {
                Ok(section) => {
                    let section = cf::typed(section);
                    let cleaned = clean_section(&name, &section);
                    if cleaned.is_empty() {
                        continue;
                    }
                    cleaned.into_cf_type()
                }
                Err(other) => other,
            };
            out.push((key, value));
        }
        out
    }
}

/// A value for `Properties::from_sections`.
#[derive(Debug, Clone, PartialEq)]
pub enum PropValue {
    /// Text.
    Str(String),
    /// Integer.
    Int(i64),
    /// Floating point.
    Float(f64),
}

fn clean_section(name: &str, section: &PropDict) -> CFRetained<PropDict> {
    let (keys, values) = section.to_vecs();
    let mut k: Vec<CFRetained<CFString>> = Vec::new();
    let mut v: Vec<CFRetained<CFType>> = Vec::new();
    for (key, value) in keys.into_iter().zip(values) {
        let kname = key.to_string();
        let drop = match name {
            "{Exif}" => kname == "PixelXDimension" || kname == "PixelYDimension",
            "{TIFF}" => kname == "Orientation" || kname == "ImageWidth" || kname == "ImageLength",
            "{IPTC}" => kname == "ImageOrientation",
            _ => false,
        };
        if !drop {
            k.push(key);
            v.push(value);
        }
    }
    let kr: Vec<&CFString> = k.iter().map(|x| &**x).collect();
    let vr: Vec<&CFType> = v.iter().map(|x| &**x).collect();
    PropDict::from_slices(&kr, &vr)
}

trait IntoCfType {
    fn into_cf_type(self) -> CFRetained<CFType>;
}

impl<T: objc2_core_foundation::Type + 'static> IntoCfType for CFRetained<T> {
    fn into_cf_type(self) -> CFRetained<CFType> {
        // SAFETY: every CF object is a CFType, so upcasting the static type is sound.
        unsafe { CFRetained::cast_unchecked(self) }
    }
}
