//! Small helpers over the CoreFoundation wrapper types. Every system constant read and every
//! type cast lives here so the rest of the crate stays free of `unsafe`.

use objc2_core_foundation::{CFBoolean, CFDictionary, CFNumber, CFRetained, CFString, CFType};

/// An ImageIO property dictionary: CFString keys, CF values.
pub(crate) type PropDict = CFDictionary<CFString, CFType>;

/// Reads a system CFString constant (`kCGImageProperty...`, `kCGColorSpace...`).
macro_rules! sys {
    ($name:path) => {
        // SAFETY: these are immutable global CFString constants exported by the system
        // frameworks. dyld binds them before any Rust code runs and they live for the whole
        // process, so reading the reference is sound.
        unsafe { $name }
    };
}
pub(crate) use sys;

/// Views an untyped dictionary returned by ImageIO as one with CFString keys.
pub(crate) fn typed(dict: CFRetained<CFDictionary>) -> CFRetained<PropDict> {
    // SAFETY: ImageIO property dictionaries are documented to have CFString keys and CF object
    // values. The cast only changes the static type parameters, not the object.
    unsafe { CFRetained::cast_unchecked(dict) }
}

pub(crate) fn get_i64(dict: &PropDict, key: &CFString) -> Option<i64> {
    let value = dict.get(key)?;
    if let Some(n) = value.downcast_ref::<CFNumber>() {
        return n.as_i64();
    }
    value
        .downcast_ref::<CFBoolean>()
        .map(|b| i64::from(b.as_bool()))
}

pub(crate) fn get_string(dict: &PropDict, key: &CFString) -> Option<String> {
    let value = dict.get(key)?;
    if let Some(s) = value.downcast_ref::<CFString>() {
        return Some(s.to_string());
    }
    if let Some(b) = value.downcast_ref::<CFBoolean>() {
        return Some(b.as_bool().to_string());
    }
    value.downcast_ref::<CFNumber>().and_then(|n| {
        n.as_i64()
            .map(|v| v.to_string())
            .or_else(|| n.as_f64().map(|v| v.to_string()))
    })
}

pub(crate) fn get_dict(dict: &PropDict, key: &CFString) -> Option<CFRetained<PropDict>> {
    let value = dict.get(key)?;
    value.downcast::<CFDictionary>().ok().map(typed)
}

/// Builds a dictionary from key and value pairs.
pub(crate) fn make_dict(entries: &[(&CFString, &CFType)]) -> CFRetained<PropDict> {
    let keys: Vec<&CFString> = entries.iter().map(|e| e.0).collect();
    let values: Vec<&CFType> = entries.iter().map(|e| e.1).collect();
    PropDict::from_slices(&keys, &values)
}

pub(crate) fn untyped(dict: &PropDict) -> &CFDictionary {
    dict.as_ref()
}

pub(crate) fn make_dict_slices(keys: &[&CFString], values: &[&CFType]) -> CFRetained<PropDict> {
    PropDict::from_slices(keys, values)
}
