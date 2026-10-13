//! The nine flow patterns from FLOWS.md.

use serde::{Deserialize, Serialize};

/// One of the nine flow patterns. The pattern decides the screen and the keyboard model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Pattern {
    /// A. Instant transform: drop, smart defaults, run.
    #[serde(rename = "A")]
    InstantTransform,
    /// B. Tuned transform: drop, live before and after preview, a few dials, run.
    #[serde(rename = "B")]
    TunedTransform,
    /// C. Visual editor: a canvas or timeline you work on directly.
    #[serde(rename = "C")]
    VisualEditor,
    /// D. Guided capture: camera or photo, rules, live checks, fix-ups.
    #[serde(rename = "D")]
    GuidedCapture,
    /// E. Generator: no file in, short form, file out.
    #[serde(rename = "E")]
    Generator,
    /// F. Instant answer: the answer appears in the bar as you type.
    #[serde(rename = "F")]
    InstantAnswer,
    /// G. Always on: runs in the background, surfaces when needed.
    #[serde(rename = "G")]
    AlwaysOn,
    /// H. Find: query, filters, results, actions on results.
    #[serde(rename = "H")]
    Find,
    /// I. Inspect: read-only report with one-click fixes.
    #[serde(rename = "I")]
    Inspect,
}

impl Pattern {
    /// Every pattern, in order.
    pub const ALL: [Pattern; 9] = [
        Pattern::InstantTransform,
        Pattern::TunedTransform,
        Pattern::VisualEditor,
        Pattern::GuidedCapture,
        Pattern::Generator,
        Pattern::InstantAnswer,
        Pattern::AlwaysOn,
        Pattern::Find,
        Pattern::Inspect,
    ];

    /// The letter used in `module.toml`.
    pub fn letter(self) -> char {
        match self {
            Self::InstantTransform => 'A',
            Self::TunedTransform => 'B',
            Self::VisualEditor => 'C',
            Self::GuidedCapture => 'D',
            Self::Generator => 'E',
            Self::InstantAnswer => 'F',
            Self::AlwaysOn => 'G',
            Self::Find => 'H',
            Self::Inspect => 'I',
        }
    }

    /// The name used in FLOWS.md.
    pub fn name(self) -> &'static str {
        match self {
            Self::InstantTransform => "Instant transform",
            Self::TunedTransform => "Tuned transform",
            Self::VisualEditor => "Visual editor",
            Self::GuidedCapture => "Guided capture",
            Self::Generator => "Generator",
            Self::InstantAnswer => "Instant answer",
            Self::AlwaysOn => "Always on",
            Self::Find => "Find",
            Self::Inspect => "Inspect",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_are_a_to_i_in_order() {
        let letters: String = Pattern::ALL.iter().map(|p| p.letter()).collect();
        assert_eq!(letters, "ABCDEFGHI");
    }

    #[test]
    fn names_match_flows_doc() {
        assert_eq!(Pattern::TunedTransform.name(), "Tuned transform");
        assert_eq!(Pattern::Inspect.name(), "Inspect");
    }

    #[test]
    fn serde_uses_letters() {
        let v = serde_json::to_string(&Pattern::Generator).unwrap();
        assert_eq!(v, "\"E\"");
        let p: Pattern = serde_json::from_str("\"H\"").unwrap();
        assert_eq!(p, Pattern::Find);
    }
}
