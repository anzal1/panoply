//! Dev-only input and accessibility self test (cargo feature `selftest-input`).
//!
//! Nothing here shows the bar. The panel is created off screen (x = -20000), never ordered front, never key.
//! GPUI frames are produced by calling `Window::draw` directly, which lays the elements out and registers the
//! input handler exactly as a normal frame does, without presenting anything.
//!
//! IME: composition is driven through the real `NSTextInputClient` selectors on GPUI's `NSView`
//! (`setMarkedText:selectedRange:replacementRange:`, `insertText:replacementRange:`, `unmarkText`, `markedRange`,
//! `selectedRange`, `hasMarkedText`, `firstRectForCharacterRange:actualRange:`), the same calls AppKit makes for
//! Hindi - Transliteration, dead keys and pinyin. Those land in GPUI's macOS bridge and from there in the kit's
//! `EntityInputHandler`. Keys go through `Window::dispatch_keystroke` (GPUI's keymap and action dispatch).
//!
//! Accessibility: the AccessKit adapter is woken the way VoiceOver wakes it (a query of the view's
//! `accessibilityChildren`), GPUI then builds its `TreeUpdate`, and the tree is read back twice: from GPUI's own
//! `debug_a11y_tree_json` and from the AppKit side through the NSAccessibility selectors of the adapter's nodes.

use crate::model::DroppedFile;
use crate::native::{self, Obj, Rect};
use crate::view::Bar;
use gpui::*;
use objc2::encode::{Encode, Encoding};
use objc2::runtime::Bool;
use objc2::{class, msg_send, sel};
use serde_json::{json, Value};
use std::ffi::{c_char, c_void, CStr};
use std::time::Duration;
use unicode_normalization::UnicodeNormalization;
use unicode_segmentation::UnicodeSegmentation;

const NOT_FOUND: u64 = i64::MAX as u64;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
struct NSRange {
    loc: u64,
    len: u64,
}
unsafe impl Encode for NSRange {
    const ENCODING: Encoding = Encoding::Struct("_NSRange", &[Encoding::ULongLong, Encoding::ULongLong]);
}
impl NSRange {
    fn invalid() -> Self {
        NSRange { loc: NOT_FOUND, len: 0 }
    }
    fn is_invalid(&self) -> bool {
        self.loc == NOT_FOUND
    }
}

// ---------- small objc helpers ----------

fn ns_string(s: &str) -> Obj {
    unsafe {
        let a: Obj = msg_send![class!(NSString), alloc];
        msg_send![a, initWithBytes: s.as_ptr() as *const c_void, length: s.len(), encoding: 4usize]
    }
}
fn ns_attributed(s: &str) -> Obj {
    unsafe {
        let a: Obj = msg_send![class!(NSAttributedString), alloc];
        msg_send![a, initWithString: ns_string(s)]
    }
}
unsafe fn describe(o: Obj) -> Option<String> {
    if o.is_null() {
        return None;
    }
    let d: Obj = msg_send![o, description];
    if d.is_null() {
        return None;
    }
    let p: *const c_char = msg_send![d, UTF8String];
    if p.is_null() {
        return None;
    }
    Some(CStr::from_ptr(p).to_string_lossy().into_owned())
}
unsafe fn responds(o: Obj, s: objc2::runtime::Sel) -> bool {
    let b: Bool = msg_send![o, respondsToSelector: s];
    b.as_bool()
}

// ---------- unicode helpers ----------

fn u16len(s: &str) -> u64 {
    s.encode_utf16().count() as u64
}

/// Clusters a caret should move by: extended grapheme clusters, plus an explicit merge across a virama
/// (consonant + virama + consonant is one conjunct) so the expectation does not depend on the segmentation
/// library's Unicode version.
fn conjunct_clusters(s: &str) -> Vec<&str> {
    let mut out: Vec<(usize, usize)> = vec![];
    for (i, g) in s.grapheme_indices(true) {
        let end = i + g.len();
        if let Some(last) = out.last_mut() {
            let prev = &s[last.0..last.1];
            let prev_virama = prev.chars().last().is_some_and(|c| unicode_normalization::char::canonical_combining_class(c) == 9);
            let next_letter = g.chars().next().is_some_and(|c| c.is_alphabetic());
            if prev_virama && next_letter {
                last.1 = end;
                continue;
            }
        }
        out.push((i, end));
    }
    out.into_iter().map(|(a, b)| &s[a..b]).collect()
}
fn plain_graphemes(s: &str) -> Vec<&str> {
    s.graphemes(true).collect()
}
fn boundaries_u16(clusters: &[&str]) -> Vec<u64> {
    let mut b = vec![0u64];
    let mut acc = 0;
    for c in clusters {
        acc += u16len(c);
        b.push(acc);
    }
    b
}

// ---------- driver ----------

struct Drv<'a> {
    handle: WindowHandle<Bar>,
    view: Obj,
    win: Obj,
    cx: &'a mut AsyncApp,
}

impl<'a> Drv<'a> {
    async fn new(handle: WindowHandle<Bar>, cx: &'a mut AsyncApp) -> Option<Drv<'a>> {
        let (view, win) = handle
            .update(cx, |_, window, _| (native::ns_view(window), native::ns_window(window)))
            .ok()?;
        Some(Drv { handle, view: view?, win: win?, cx })
    }

    /// Window-level work that must not hold the root `Bar` lease (draw renders the Bar, key listeners update it).
    fn win<R>(&mut self, f: impl FnOnce(&mut Window, &mut App) -> R) -> Option<R> {
        let any: AnyWindowHandle = self.handle.into();
        self.cx.update_window(any, |_, window, cx| f(window, cx)).ok()
    }
    fn draw(&mut self) {
        let _ = self.win(|window, cx| window.draw(cx).clear(cx));
    }
    async fn settle(&mut self) {
        self.cx.background_executor().timer(Duration::from_millis(15)).await;
        self.draw();
    }
    fn reset(&mut self) {
        let _ = self.handle.update(self.cx, |bar, window, cx| {
            bar.selftest_reset(window, cx);
        });
        self.draw();
        self.draw();
    }
    fn key(&mut self, k: &str) {
        let ks = Keystroke::parse(k).expect("keystroke");
        let _ = self.win(move |window, cx| window.dispatch_keystroke(ks, cx));
        self.draw();
    }

    // NSTextInputClient, called on the real view. AppKit passes {NSNotFound, 0} when it has no replacement range.
    fn has_marked(&self) -> bool {
        unsafe {
            let b: Bool = msg_send![self.view, hasMarkedText];
            b.as_bool()
        }
    }
    fn marked(&self) -> NSRange {
        unsafe { msg_send![self.view, markedRange] }
    }
    fn selected(&self) -> NSRange {
        unsafe { msg_send![self.view, selectedRange] }
    }
    fn set_marked(&mut self, text: &str, sel: (u64, u64)) {
        unsafe {
            let _: () = msg_send![self.view, setMarkedText: ns_attributed(text), selectedRange: NSRange { loc: sel.0, len: sel.1 }, replacementRange: NSRange::invalid()];
        }
        self.draw();
    }
    fn insert(&mut self, text: &str) {
        unsafe {
            let _: () = msg_send![self.view, insertText: ns_string(text), replacementRange: NSRange::invalid()];
        }
        self.draw();
    }
    fn unmark(&mut self) {
        unsafe {
            let _: () = msg_send![self.view, unmarkText];
        }
        self.draw();
    }
    fn first_rect(&self, r: NSRange) -> Rect {
        unsafe {
            let mut actual = NSRange::invalid();
            msg_send![self.view, firstRectForCharacterRange: r, actualRange: &mut actual as *mut NSRange as *mut c_void]
        }
    }
    fn substring(&self, r: NSRange) -> Option<String> {
        unsafe {
            let mut actual = NSRange::invalid();
            let s: Obj = msg_send![self.view, attributedSubstringForProposedRange: r, actualRange: &mut actual as *mut NSRange as *mut c_void];
            if s.is_null() {
                return None;
            }
            let plain: Obj = msg_send![s, string];
            describe(plain)
        }
    }

    fn value(&mut self) -> String {
        self.handle.update(self.cx, |bar, _, cx| bar.input_entity().read(cx).value().to_string()).unwrap_or_default()
    }
    fn selected_value(&mut self) -> String {
        self.handle.update(self.cx, |bar, _, cx| bar.input_entity().read(cx).selected_value().to_string()).unwrap_or_default()
    }
    fn ui(&mut self) -> (String, usize, usize) {
        self.handle
            .update(self.cx, |bar, _, cx| {
                let v = bar.input_entity().read(cx).value().to_string();
                let rows_for = bar.rows_for(&bar.query.clone());
                let _ = v;
                (bar.query.clone(), bar.row_count(), rows_for)
            })
            .unwrap_or_default()
    }
    fn rows_for(&mut self, q: String) -> usize {
        self.handle.update(self.cx, move |bar, _, _| bar.rows_for(&q)).unwrap_or(0)
    }
    fn window_json(&self) -> Value {
        let f = native::frame(self.win);
        json!({
            "visible": native::is_visible(self.win),
            "key": native::is_key(self.win),
            "frame_origin": [f.o.x, f.o.y],
            "frame_size": [f.s.w, f.s.h],
            "alpha": native::alpha(self.win),
        })
    }
}

fn chk(list: &mut Vec<Value>, name: &str, pass: bool, detail: impl Into<Value>) {
    list.push(json!({"check": name, "pass": pass, "detail": detail.into()}));
}

// ---------- IME cases ----------

enum Step {
    Mark(&'static str),
    Insert(&'static str),
    Unmark,
}
use Step::*;

struct Case {
    name: &'static str,
    note: &'static str,
    steps: Vec<Step>,
    expected: &'static str,
    /// Accepted values after one Cmd+Z (the whole composed word gone in one step).
    undo_to: Vec<&'static str>,
}

fn cases() -> Vec<Case> {
    vec![
        Case {
            name: "hindi_kshetra",
            note: "kshetra -> क्षेत्र, Hindi - Transliteration style: marked text grows, Return commits with insertText",
            steps: vec![Mark("क"), Mark("कस"), Mark("क्ष"), Mark("क्षे"), Mark("क्षेत"), Mark("क्षेत्र"), Insert("क्षेत्र")],
            expected: "क्षेत्र",
            undo_to: vec![""],
        },
        Case {
            name: "hindi_shree",
            note: "shree -> श्री",
            steps: vec![Mark("श"), Mark("श्"), Mark("श्र"), Mark("श्री"), Insert("श्री")],
            expected: "श्री",
            undo_to: vec![""],
        },
        Case {
            name: "hindi_shraddha",
            note: "shraddha -> श्रद्धा",
            steps: vec![Mark("श"), Mark("श्र"), Mark("श्रद"), Mark("श्रद्"), Mark("श्रद्ध"), Mark("श्रद्धा"), Insert("श्रद्धा")],
            expected: "श्रद्धा",
            undo_to: vec![""],
        },
        Case {
            name: "hindi_namaste_duniya",
            note: "two words; Space commits word 1 (insertText), a plain space is typed, word 2 composed and committed; one undo removes only the last word",
            steps: vec![
                Mark("न"), Mark("नम"), Mark("नमस"), Mark("नमस्"), Mark("नमस्त"), Mark("नमस्ते"), Insert("नमस्ते"),
                Insert(" "),
                Mark("द"), Mark("दु"), Mark("दुन"), Mark("दुनि"), Mark("दुनिय"), Mark("दुनिया"), Insert("दुनिया"),
            ],
            expected: "नमस्ते दुनिया",
            undo_to: vec!["नमस्ते ", "नमस्ते"],
        },
        Case {
            name: "bengali_kshama",
            note: "ক্ষমা (ksha + ma + aa-kar), conjunct ক্ষ",
            steps: vec![Mark("ক"), Mark("ক্"), Mark("ক্ষ"), Mark("ক্ষম"), Mark("ক্ষমা"), Insert("ক্ষমা")],
            expected: "ক্ষমা",
            undo_to: vec![""],
        },
        Case {
            name: "bengali_shree",
            note: "শ্রী",
            steps: vec![Mark("শ"), Mark("শ্"), Mark("শ্র"), Mark("শ্রী"), Insert("শ্রী")],
            expected: "শ্রী",
            undo_to: vec![""],
        },
        Case {
            name: "deadkey_acute_e",
            note: "Option+E marks the spacing acute, E replaces it with é (precomposed U+00E9)",
            steps: vec![Mark("\u{b4}"), Insert("\u{e9}")],
            expected: "\u{e9}",
            undo_to: vec![""],
        },
        Case {
            name: "pinyin_nihao",
            note: "nihao -> 你好, marked pinyin with a syllable break, candidate commit with insertText",
            steps: vec![Mark("n"), Mark("ni"), Mark("ni h"), Mark("ni ha"), Mark("ni hao"), Insert("你好")],
            expected: "你好",
            undo_to: vec![""],
        },
        Case {
            name: "paste_conjuncts",
            note: "paste of क्षत्रिय श्रीमान. The system pasteboard is not touched; the text enters through insertText with no marked range, which is the call the kit's Paste ends in",
            steps: vec![Insert("क्षत्रिय श्रीमान")],
            expected: "क्षत्रिय श्रीमान",
            undo_to: vec![""],
        },
        Case {
            name: "shrink_then_unmark",
            note: "composition shrinks (IME Backspace) then commits through unmarkText instead of insertText: क्षेत्र -> क्षेत्र -> क्षेत -> क्ष, unmark keeps क्ष",
            steps: vec![Mark("क्षेत्र"), Mark("क्षेत"), Mark("क्ष"), Unmark],
            expected: "क्ष",
            undo_to: vec![""],
        },
        Case {
            name: "latin_filter_control",
            note: "control for list filtering: composing then committing an ASCII query must filter the tool list",
            steps: vec![Mark("c"), Mark("com"), Mark("compress"), Insert("compress")],
            expected: "compress",
            undo_to: vec![""],
        },
    ]
}

async fn run_case(d: &mut Drv<'_>, c: &Case) -> Value {
    let mut checks: Vec<Value> = vec![];
    let mut trace: Vec<Value> = vec![];
    d.reset();

    let mut prefix = String::new();
    let mut comp_start: Option<u64> = None;
    let mut first_rect: Option<Rect> = None;
    let mut last_rect: Option<Rect> = None;
    let frame = native::frame(d.win);

    for (i, step) in c.steps.iter().enumerate() {
        match step {
            Mark(t) => {
                if comp_start.is_none() {
                    comp_start = Some(d.selected().loc);
                    prefix = d.value();
                }
                let start = comp_start.unwrap();
                let tlen = u16len(t);
                d.set_marked(t, (tlen, 0));
                let m = d.marked();
                let sel = d.selected();
                let v = d.value();
                let want_val = format!("{prefix}{t}");
                let ok_range = d.has_marked() && m == NSRange { loc: start, len: tlen };
                let ok_val = v == want_val;
                let ok_caret = sel.loc >= start && sel.loc <= start + tlen;
                let r = d.first_rect(m);
                if first_rect.is_none() {
                    first_rect = Some(r);
                }
                last_rect = Some(r);
                let (q, rows, _) = d.ui();
                trace.push(json!({"step": i, "op": "setMarkedText", "text": t, "marked": [m.loc, m.len], "selected": [sel.loc, sel.len], "value": v, "ui_query": q, "ui_rows": rows}));
                chk(&mut checks, &format!("marked_range_reported[{i}]"), ok_range, format!("want ({start},{tlen}) got ({},{}) has_marked={}", m.loc, m.len, d.has_marked()));
                chk(&mut checks, &format!("marked_text_in_buffer[{i}]"), ok_val, format!("want {want_val:?} got {v:?}"));
                chk(&mut checks, &format!("caret_inside_marked[{i}]"), ok_caret, format!("caret {} within {}..{}", sel.loc, start, start + tlen));
            }
            Insert(t) => {
                let was_composing = comp_start.is_some();
                let want_val = if was_composing { format!("{prefix}{t}") } else { format!("{}{t}", d.value()) };
                d.insert(t);
                let v = d.value();
                let m = d.marked();
                let cleared = !d.has_marked() && m.is_invalid();
                trace.push(json!({"step": i, "op": "insertText", "text": t, "composing": was_composing, "marked_after": if m.is_invalid() {Value::Null} else {json!([m.loc, m.len])}, "value": v}));
                chk(&mut checks, &format!("commit_value[{i}]"), v == want_val, format!("want {want_val:?} got {v:?}"));
                chk(&mut checks, &format!("marked_cleared_after_insert[{i}]"), cleared, format!("has_marked={} markedRange=({},{})", d.has_marked(), m.loc, m.len));
                comp_start = None;
                prefix = String::new();
            }
            Unmark => {
                let before = d.value();
                d.unmark();
                let v = d.value();
                let m = d.marked();
                trace.push(json!({"step": i, "op": "unmarkText", "value": v}));
                chk(&mut checks, &format!("unmark_keeps_text[{i}]"), v == before, format!("before {before:?} after {v:?}"));
                chk(&mut checks, &format!("marked_cleared_after_unmark[{i}]"), !d.has_marked() && m.is_invalid(), format!("has_marked={} markedRange=({},{})", d.has_marked(), m.loc, m.len));
                comp_start = None;
                prefix = String::new();
            }
        }
    }

    // Candidate window anchor: firstRectForCharacterRange must land on the field, not at the screen origin.
    if let (Some(a), Some(b)) = (first_rect, last_rect) {
        let inside = |r: &Rect| {
            r.s.h > 0.0
                && r.o.x >= frame.o.x - 1.0
                && r.o.x <= frame.o.x + frame.s.w + 1.0
                && r.o.y >= frame.o.y - 1.0
                && r.o.y <= frame.o.y + frame.s.h + 1.0
        };
        chk(
            &mut checks,
            "candidate_rect_on_field",
            inside(&a) && inside(&b),
            json!({"first": [a.o.x, a.o.y, a.s.w, a.s.h], "last": [b.o.x, b.o.y, b.s.w, b.s.h], "window_frame": [frame.o.x, frame.o.y, frame.s.w, frame.s.h],
                   "note": "window is off screen on purpose, so this proves the rect follows the field inside the window frame, not that the OS candidate strip is placed well"}),
        );
    }

    // Committed text.
    let got = d.value();
    let nfc: String = got.nfc().collect();
    chk(&mut checks, "text_equals_expected", got == c.expected, format!("want {:?} got {:?}", c.expected, got));
    chk(&mut checks, "text_is_nfc", got == nfc && c.expected.nfc().collect::<String>() == c.expected, format!("nfc(got)=={}", nfc == got));

    // List filtering re-ran after the commit.
    let (q, rows, _) = d.ui();
    let want_rows = d.rows_for(got.clone());
    chk(&mut checks, "list_filter_reran_after_commit", q == got && rows == want_rows, json!({"bar_query": q, "rows_now": rows, "rows_for_committed_text": want_rows}));
    if c.name == "latin_filter_control" {
        chk(&mut checks, "latin_control_has_matches", rows > 0, json!({"rows": rows}));
    }

    // Undo as one step, then redo.
    d.key("cmd-z");
    let after_undo = d.value();
    let undo_ok = c.undo_to.iter().any(|s| *s == after_undo);
    chk(&mut checks, "cmd_z_undoes_whole_word_in_one_step", undo_ok, json!({"after_one_undo": after_undo, "accepted": c.undo_to}));
    d.key("cmd-shift-z");
    let after_redo = d.value();
    chk(&mut checks, "cmd_shift_z_redo_restores", after_redo == c.expected, json!({"after_redo": after_redo}));
    d.key("end");

    // Caret movement by cluster.
    let text = d.value();
    let conj = conjunct_clusters(&text);
    let plain = plain_graphemes(&text);
    let b_conj = boundaries_u16(&conj);
    let b_plain = boundaries_u16(&plain);
    let total = u16len(&text);
    if text == c.expected && !text.is_empty() {
        // Left from the end.
        let mut left = vec![d.selected().loc];
        for _ in 0..80 {
            d.key("left");
            let s = d.selected();
            let c0 = s.loc;
            if s.len != 0 || Some(&c0) == left.last() {
                break;
            }
            left.push(c0);
            if c0 == 0 {
                break;
            }
        }
        let want_left: Vec<u64> = b_conj.iter().rev().copied().collect();
        let inside: Vec<u64> = left.iter().copied().filter(|p| !b_conj.contains(p)).collect();
        chk(&mut checks, "caret_left_by_cluster", left == want_left && left.first() == Some(&total), json!({"stops_utf16": left, "expected_stops": want_left, "stops_inside_a_cluster": inside, "plain_grapheme_stops": b_plain.iter().rev().copied().collect::<Vec<_>>()}));
        // Right back to the end.
        let mut right = vec![d.selected().loc];
        for _ in 0..80 {
            d.key("right");
            let s = d.selected();
            if Some(&s.loc) == right.last() {
                break;
            }
            right.push(s.loc);
            if s.loc == total {
                break;
            }
        }
        let inside_r: Vec<u64> = right.iter().copied().filter(|p| !b_conj.contains(p)).collect();
        chk(&mut checks, "caret_right_by_cluster", right == b_conj, json!({"stops_utf16": right, "expected_stops": b_conj, "stops_inside_a_cluster": inside_r}));

        // Shift+Left selects exactly the last cluster.
        d.key("end");
        d.key("shift-left");
        let sel = d.selected();
        let last = conj.last().copied().unwrap_or("");
        let want_len = u16len(last);
        let sel_txt = d.selected_value();
        let via_ns = d.substring(sel);
        chk(
            &mut checks,
            "shift_left_selects_last_cluster",
            sel.len == want_len && sel.loc == total - want_len && sel_txt == last,
            json!({"selected_range": [sel.loc, sel.len], "selected_text": sel_txt, "ns_substring": via_ns, "want_cluster": last}),
        );
        d.key("end");

        // Backspace removes one whole cluster.
        d.key("backspace");
        let after_bs = d.value();
        let want_prefix: String = conj[..conj.len() - 1].concat();
        let plain_prefix: String = plain[..plain.len() - 1].concat();
        chk(
            &mut checks,
            "backspace_removes_one_whole_cluster",
            after_bs == want_prefix,
            json!({"after_backspace": after_bs, "want": want_prefix, "plain_grapheme_prefix": plain_prefix, "removed_chars": text.chars().count() as i64 - after_bs.chars().count() as i64}),
        );
        d.key("cmd-z");
        let after_bs_undo = d.value();
        chk(&mut checks, "cmd_z_restores_after_backspace", after_bs_undo == text, json!({"after": after_bs_undo}));
    }

    let pass = checks.iter().all(|c| c["pass"] == true);
    json!({
        "case": c.name,
        "note": c.note,
        "pass": pass,
        "expected": c.expected,
        "got": got,
        "expected_utf16_len": u16len(c.expected),
        "conjunct_aware_clusters": conj,
        "plain_unicode_segmentation_clusters": plain,
        "checks": checks,
        "failed": checks.iter().filter(|c| c["pass"] != true).map(|c| c["check"].clone()).collect::<Vec<_>>(),
        "trace": trace,
    })
}

async fn run_ime(d: &mut Drv<'_>) -> Value {
    let mut out = vec![];
    for c in cases() {
        out.push(run_case(d, &c).await);
    }
    let passed = out.iter().filter(|c| c["pass"] == true).count();
    json!({
        "test": "imetest",
        "kit": {"gpui-pre": "0.3.9", "gpui-base": "0.7.3", "accesskit_macos": "0.26.3"},
        "driver": "NSTextInputClient selectors on GPUI's NSView + Window::dispatch_keystroke; no OS input method, no pasteboard, no window shown",
        "window": d.window_json(),
        "summary": {"passed": passed, "total": out.len()},
        "cases": out,
    })
}

// ---------- keys the bar owns ----------

async fn run_keys(d: &mut Drv<'_>) -> Value {
    let mut checks = vec![];
    d.reset();
    d.insert("pdf");
    d.settle().await;
    let (q, rows, _) = d.ui();
    chk(&mut checks, "typed_ascii_filters_list", q == "pdf" && rows > 1, json!({"query": q, "rows": rows}));
    let sel = |d: &mut Drv| d.handle.update(d.cx, |bar, _, _| bar.selected).unwrap_or(99);
    let s0 = sel(d);
    d.key("down");
    let s1 = sel(d);
    d.key("up");
    let s2 = sel(d);
    d.key("ctrl-n");
    let s3 = sel(d);
    d.key("ctrl-p");
    let s4 = sel(d);
    chk(&mut checks, "down_up_propagate_through_kit_input_to_bar", (s0, s1, s2) == (0, 1, 0), json!({"start": s0, "after_down": s1, "after_up": s2}));
    chk(&mut checks, "ctrl_n_ctrl_p_propagate", (s3, s4) == (1, 0), json!({"after_ctrl_n": s3, "after_ctrl_p": s4}));
    d.key("enter");
    let msg = d.handle.update(d.cx, |bar, _, _| bar.footer_msg.clone()).unwrap_or(None);
    chk(&mut checks, "enter_runs_selected_tool", msg.as_deref().is_some_and(|m| m.starts_with("Opened ")), json!({"footer_msg": msg}));
    d.key("escape");
    let (q2, _, _) = d.ui();
    let v2 = d.value();
    chk(&mut checks, "escape_clears_query_first", q2.is_empty() && v2.is_empty(), json!({"bar_query": q2, "input_value": v2}));
    let pass = checks.iter().all(|c| c["pass"] == true);
    json!({"test": "keystest", "pass": pass, "window": d.window_json(), "checks": checks})
}

// ---------- shaping ----------

const SHAPE_TEXT: &str = "क्षेत्र श्री श्रद्धा নমস্কার ক্ষমা";

fn glyphs_in(line: &ShapedLine, a: usize, b: usize) -> Vec<(u32, usize)> {
    let mut v = vec![];
    for run in &line.runs {
        for g in &run.glyphs {
            if g.index >= a && g.index < b {
                v.push((g.id.0, g.index));
            }
        }
    }
    v
}

async fn run_shape(d: &mut Drv<'_>) -> Value {
    d.reset();
    // 1. Shaping through GPUI's text system, the same call the kit's field layout makes.
    let shaped = d
        .handle
        .update(d.cx, |_, window, _| {
            let ts = window.text_system().clone();
            let style = window.text_style();
            let font = style.font();
            let size = px(20.);
            let mk = |t: &str| {
                let run = TextRun { len: t.len(), font: font.clone(), color: black(), background_color: None, underline: None, strikethrough: None };
                ts.shape_line(t.to_string().into(), size, &[run], None)
            };
            let line = mk(SHAPE_TEXT);
            let virama_dev = mk("\u{94d}");
            let virama_ben = mk("\u{9cd}");
            let ka_dev = mk("\u{915}");
            let fam = font.family.to_string();
            let fid = |l: &ShapedLine| l.runs.iter().flat_map(|r| r.glyphs.iter().map(|g| g.id.0)).collect::<Vec<_>>();
            let font_names: Vec<Value> = line
                .runs
                .iter()
                .map(|r| {
                    let f = ts.get_font_for_id(r.font_id);
                    json!({"font_id": format!("{:?}", r.font_id), "family": f.map(|f| f.family.to_string()), "glyph_count": r.glyphs.len()})
                })
                .collect();
            let all: Vec<(u32, usize, f32)> = line.runs.iter().flat_map(|r| r.glyphs.iter().map(|g| (g.id.0, g.index, g.position.x.as_f32()))).collect();
            let conj_list = ["क्ष", "त्र", "श्र", "द्ध", "স্ক", "ক্ষ", "শ্র"];
            let mut conjuncts = vec![];
            // CoreText shapes a lone virama as [placeholder, virama]; only the last glyph is the visible halant.
            let virama_ids_dev: Vec<u32> = fid(&virama_dev).into_iter().last().into_iter().collect();
            let virama_ids_ben: Vec<u32> = fid(&virama_ben).into_iter().last().into_iter().collect();
            for cj in conj_list {
                let mut from = 0;
                while let Some(p) = SHAPE_TEXT[from..].find(cj) {
                    let a = from + p;
                    let b = a + cj.len();
                    let gl = glyphs_in(&line, a, b);
                    let ids: Vec<u32> = gl.iter().map(|g| g.0).collect();
                    let idxs: Vec<usize> = gl.iter().map(|g| g.1).collect();
                    let scalars = cj.chars().count();
                    let virama_visible = ids.iter().any(|i| virama_ids_dev.contains(i) || virama_ids_ben.contains(i));
                    // A glyph index (byte offset) strictly inside the conjunct means the shaper started a new cluster inside it.
                    let inner_starts: Vec<usize> = idxs.iter().copied().filter(|i| *i > a && *i < b).collect();
                    conjuncts.push(json!({
                        "conjunct": cj, "byte_range": [a, b], "scalars": scalars, "glyph_ids": ids, "glyph_count": gl.len(),
                        "glyph_start_indices": idxs, "forms_single_glyph": gl.len() == 1, "fewer_glyphs_than_scalars": gl.len() < scalars,
                        "glyph_starts_inside_conjunct": inner_starts, "lone_virama_glyph_present": virama_visible,
                    }));
                    from = b;
                }
            }
            // clusters per word
            let mut words = vec![];
            let mut off = 0;
            for w in SHAPE_TEXT.split(' ') {
                let (a, b) = (off, off + w.len());
                let gl = glyphs_in(&line, a, b);
                let mut starts: Vec<usize> = gl.iter().map(|g| g.1).collect();
                starts.sort_unstable();
                starts.dedup();
                let mut clusters = vec![];
                for (k, s) in starts.iter().enumerate() {
                    let e = starts.get(k + 1).copied().unwrap_or(b);
                    clusters.push(json!({"text": &SHAPE_TEXT[*s..e], "byte_start": s, "glyph_ids": gl.iter().filter(|g| g.1 == *s).map(|g| g.0).collect::<Vec<_>>()}));
                }
                words.push(json!({"word": w, "scalars": w.chars().count(), "glyph_count": gl.len(), "shaper_clusters": clusters}));
                off = b + 1;
            }
            json!({
                "text": SHAPE_TEXT, "font_family_requested": fam, "font_size_px": 20, "scale_factor": window.scale_factor(),
                "runs": font_names, "total_glyphs": all.len(), "scalars": SHAPE_TEXT.chars().count(),
                "glyphs": all.iter().map(|(id, idx, x)| json!({"id": id, "byte_index": idx, "x": (*x * 100.0).round() / 100.0})).collect::<Vec<_>>(),
                "visible_halant_glyph_ids": {"devanagari_U+094D": virama_ids_dev, "bengali_U+09CD": virama_ids_ben, "devanagari_ka": fid(&ka_dev)},
                "words": words, "conjuncts": conjuncts, "width_px": line.width.as_f32(),
            })
        })
        .unwrap_or(json!({"error": "window"}));

    // 2. Render the real field (kit Input in the bar's window) off screen at 3x device pixel size.
    let path = crate::spec_folder().join("gpui-checks").join("hindi-field.png");
    let _ = std::fs::create_dir_all(path.parent().unwrap());
    let scale = d.handle.update(d.cx, |_, window, _| window.scale_factor()).unwrap_or(1.0);
    let zoom = 3.0 / scale;
    let _ = d.handle.update(d.cx, move |bar, window, cx| {
        bar.zoom = zoom;
        bar.selftest_reset(window, cx);
        let input = bar.input_entity();
        input.update(cx, |s, cx| s.set_value(SHAPE_TEXT.to_string(), window, cx));
        cx.notify();
    });
    d.settle().await;
    d.draw();
    let img = d.win(|window, _| window.render_to_image()).ok_or_else(|| anyhow::anyhow!("window"));
    let png = match img {
        Ok(Ok(img)) => match {
            // Crop the opaque card to the text row so the PNG is mostly glyphs.
            let (cw, ch) = (img.width().min(760), 150u32.min(img.height()));
            let cy = (img.height() / 2).saturating_sub(ch / 2 + 20);
            let img2 = image::imageops::crop_imm(&img, 0, cy, cw, ch).to_image();
            img2.save(&path).map(|_| img2)
        } {
            Ok(img) => json!({"ok": true, "path": path.to_string_lossy(), "w": img.width(), "h": img.height(), "backing_scale": scale, "text_size_logical_px": 20.0 * zoom, "device_px_per_em": 20.0 * zoom * scale}),
            Err(e) => json!({"ok": false, "error": e.to_string()}),
        },
        Ok(Err(e)) => json!({"ok": false, "error": e.to_string()}),
        Err(e) => json!({"ok": false, "error": e.to_string()}),
    };
    let _ = d.handle.update(d.cx, |bar, window, cx| {
        bar.zoom = 1.0;
        bar.selftest_reset(window, cx);
    });
    d.draw();
    json!({"test": "shapetest", "window": d.window_json(), "shaping": shaped, "png": png})
}

// ---------- accessibility ----------

unsafe fn find_el(el: Obj, label: &str, depth: usize) -> Obj {
    let t: Obj = msg_send![el, accessibilityTitle];
    if describe(t).as_deref() == Some(label) {
        return el;
    }
    let kids: Obj = msg_send![el, accessibilityChildren];
    if !kids.is_null() && depth < 8 {
        let n: usize = msg_send![kids, count];
        for i in 0..n {
            let k: Obj = msg_send![kids, objectAtIndex: i];
            let f = find_el(k, label, depth + 1);
            if !f.is_null() {
                return f;
            }
        }
    }
    std::ptr::null_mut()
}

unsafe fn ax_node(el: Obj, depth: usize) -> Value {
    let mut m = serde_json::Map::new();
    let s = |sel: objc2::runtime::Sel| -> Option<String> {
        if responds(el, sel) {
            let o: Obj = msg_send![el, performSelector: sel];
            describe(o)
        } else {
            None
        }
    };
    m.insert("role".into(), json!(s(sel!(accessibilityRole))));
    m.insert("subrole".into(), json!(s(sel!(accessibilitySubrole))));
    m.insert("label".into(), json!(s(sel!(accessibilityTitle))));
    m.insert("help".into(), json!(s(sel!(accessibilityHelp))));
    m.insert("value".into(), json!(s(sel!(accessibilityValue))));
    m.insert("placeholder".into(), json!(s(sel!(accessibilityPlaceholderValue))));
    let sel_b: Bool = msg_send![el, isAccessibilitySelected];
    m.insert("selected".into(), json!(sel_b.as_bool()));
    if responds(el, sel!(accessibilitySelectedTextRange)) {
        let r: NSRange = msg_send![el, accessibilitySelectedTextRange];
        m.insert("selected_text_range".into(), json!([r.loc, r.len]));
        let n: isize = msg_send![el, accessibilityNumberOfCharacters];
        m.insert("number_of_characters".into(), json!(n));
    }
    let press: Bool = msg_send![el, isAccessibilitySelectorAllowed: sel!(accessibilityPerformPress)];
    m.insert("press_allowed".into(), json!(press.as_bool()));
    let focusable: Bool = msg_send![el, isAccessibilitySelectorAllowed: sel!(setAccessibilityFocused:)];
    m.insert("focusable".into(), json!(focusable.as_bool()));
    let f: Rect = msg_send![el, accessibilityFrame];
    m.insert("frame".into(), json!([f.o.x, f.o.y, f.s.w, f.s.h]));
    let kids: Obj = msg_send![el, accessibilityChildren];
    let mut children = vec![];
    if !kids.is_null() && depth < 8 {
        let n: usize = msg_send![kids, count];
        for i in 0..n {
            let k: Obj = msg_send![kids, objectAtIndex: i];
            children.push(ax_node(k, depth + 1));
        }
    }
    m.insert("children".into(), Value::Array(children));
    Value::Object(m)
}


fn flatten_gpui(tree: &Value) -> Vec<Value> {
    let mut out = vec![];
    let Some(nodes) = tree["nodes"].as_object() else { return out };
    for (id, n) in nodes {
        let a = &n["aria"];
        out.push(json!({
            "id": id,
            "role": a["role"],
            "label": a["label"],
            "description": a["description"],
            "value": a["value"],
            "placeholder": a["placeholder"],
            "selected": a["selected"],
            "position_in_set": a["position_in_set"],
            "size_of_set": a["size_of_set"],
            "keyboard_shortcut": a["keyboard_shortcut"],
            "on_action": a["on_action"],
            "children": n["children"],
        }));
    }
    out
}

fn by_role<'a>(nodes: &'a [Value], role: &str) -> Vec<&'a Value> {
    nodes.iter().filter(|n| n["role"] == role).collect()
}

async fn dump_state(d: &mut Drv<'_>, label: &str) -> Value {
    d.settle().await;
    d.draw();
    let tree: Option<String> = d.win(|window, _| window.debug_a11y_tree_json()).unwrap_or(None);
    let parsed: Value = tree.as_deref().and_then(|t| serde_json::from_str(t).ok()).unwrap_or(Value::Null);
    json!({"state": label, "gpui_tree_frame": parsed["frame"], "gpui_focus": parsed["gpui_focus"], "root": parsed["root"], "nodes": flatten_gpui(&parsed)})
}

async fn run_a11y(d: &mut Drv<'_>) -> Value {
    d.reset();
    // Wake the adapter the way an assistive technology does: ask the content view for its accessibility children.
    let cv = native::content_view(d.win);
    let (active_before, kids_n) = unsafe {
        let kids: Obj = msg_send![cv, accessibilityChildren];
        let n: usize = if kids.is_null() { 0 } else { msg_send![kids, count] };
        (false, n)
    };
    let _ = active_before;
    d.settle().await;
    d.cx.background_executor().timer(Duration::from_millis(60)).await;
    d.draw();
    let active = d.win(|window, _| window.is_a11y_active()).unwrap_or(false);
    let enabled = d.win(|window, _| window.is_a11y_enabled()).unwrap_or(false);

    // State A: ASCII typed through the IME path, a dropped file so the remove button exists.
    let _ = d.handle.update(d.cx, |bar, _, cx| bar.selftest_set_file(Some(DroppedFile::from_path(std::path::Path::new("report.pdf"))), cx));
    d.insert("pdf");
    let a = dump_state(d, "ascii_query_pdf_with_file_chip").await;
    let sel_a: Vec<Value> = by_role(a["nodes"].as_array().unwrap(), "ListBoxOption").iter().map(|n| json!({"label": n["label"], "selected": n["selected"], "pos": n["position_in_set"]})).collect();

    // State B: arrow down, selection must move in the tree.
    d.key("down");
    let b = dump_state(d, "after_down_key").await;
    let sel_b: Vec<Value> = by_role(b["nodes"].as_array().unwrap(), "ListBoxOption").iter().map(|n| json!({"label": n["label"], "selected": n["selected"], "pos": n["position_in_set"]})).collect();

    // State C: Hindi text in the field (committed through the IME path).
    d.key("cmd-a");
    d.insert("क्षेत्र");
    let c = dump_state(d, "devanagari_query").await;

    // Back to state A for the AppKit-side read.
    d.key("cmd-a");
    d.insert("pdf");
    d.draw();
    d.settle().await;

    let appkit = unsafe {
        let kids: Obj = msg_send![cv, accessibilityChildren];
        let mut roots = vec![];
        if !kids.is_null() {
            let n: usize = msg_send![kids, count];
            for i in 0..n {
                let k: Obj = msg_send![kids, objectAtIndex: i];
                roots.push(ax_node(k, 0));
            }
        }
        let focus: Obj = msg_send![cv, accessibilityFocusedUIElement];
        let focus_desc = if focus.is_null() { None } else { Some(ax_node(focus, 7)) };
        json!({"content_view_children": roots, "focused_element": focus_desc})
    };

    // Verdict on the GPUI tree (state A).
    let na = a["nodes"].as_array().unwrap();
    let interactive = ["SearchInput", "TextInput", "Button", "ListBoxOption"];
    let mut missing = vec![];
    for n in na {
        let r = n["role"].as_str().unwrap_or("");
        if interactive.contains(&r) {
            let lab = n["label"].as_str().unwrap_or("");
            if lab.trim().is_empty() {
                missing.push(json!({"id": n["id"], "role": r, "problem": "no label"}));
            }
        }
    }
    let field = by_role(na, "SearchInput");
    let field_value_a = field.first().map(|f| f["value"].clone());
    let field_value_c = by_role(c["nodes"].as_array().unwrap(), "SearchInput").first().map(|f| f["value"].clone());
    let buttons: Vec<Value> = by_role(na, "Button").iter().map(|n| json!({"label": n["label"], "actions": n["on_action"], "shortcut": n["keyboard_shortcut"]})).collect();
    let listbox = by_role(na, "ListBox");
    let rows = by_role(na, "ListBoxOption");
    let selected_flags_a: Vec<bool> = rows.iter().map(|n| n["selected"] == true).collect();
    let selected_flags_b: Vec<bool> = by_role(b["nodes"].as_array().unwrap(), "ListBoxOption").iter().map(|n| n["selected"] == true).collect();

    // AppKit side: find the field and read value + selected text range.
    fn find_role<'a>(n: &'a Value, role: &str, out: &mut Vec<&'a Value>) {
        if n["role"] == role {
            out.push(n);
        }
        if let Some(k) = n["children"].as_array() {
            for c in k {
                find_role(c, role, out);
            }
        }
    }
    let mut tf = vec![];
    let mut btn = vec![];
    let mut st = vec![];
    for r in appkit["content_view_children"].as_array().unwrap() {
        find_role(r, "AXTextField", &mut tf);
        find_role(r, "AXButton", &mut btn);
        find_role(r, "AXStaticText", &mut st);
    }
    let ak_field = tf.first().cloned().cloned().unwrap_or(Value::Null);

    let mut checks = vec![];
    chk(&mut checks, "a11y_adapter_activated_without_screen_reader", active && enabled, json!({"is_a11y_active": active, "enabled": enabled, "initial_children_returned": kids_n}));
    chk(&mut checks, "search_field_has_role_and_label", field.len() == 1 && field[0]["label"] == "Search tools", json!(field));
    chk(&mut checks, "search_field_exposes_value_gpui", field_value_a == Some(json!("pdf")) && field_value_c == Some(json!("क्षेत्र")), json!({"typed_ascii": field_value_a, "typed_devanagari": field_value_c}));
    chk(&mut checks, "search_field_exposes_value_appkit", ak_field["value"] == "pdf" && ak_field["role"] == "AXTextField", json!({"role": ak_field["role"], "label": ak_field["label"], "value": ak_field["value"], "placeholder": ak_field["placeholder"]}));
    chk(&mut checks, "result_list_role_and_label", listbox.len() == 1 && listbox[0]["label"] == "Tools", json!(listbox));
    chk(&mut checks, "rows_have_role_label_and_selected_state", !rows.is_empty() && rows.iter().all(|r| r["label"].as_str().is_some_and(|s| !s.is_empty()) && r["selected"].is_boolean()) && selected_flags_a.iter().filter(|s| **s).count() == 1, json!({"row_count": rows.len(), "selected_flags": selected_flags_a}));
    chk(&mut checks, "selected_state_follows_arrow_key", selected_flags_a.first() == Some(&true) && selected_flags_b.get(1) == Some(&true) && selected_flags_b.first() == Some(&false), json!({"before": sel_a.iter().take(3).collect::<Vec<_>>(), "after_down": sel_b.iter().take(3).collect::<Vec<_>>()}));
    chk(&mut checks, "footer_buttons_have_role_and_label", buttons.iter().any(|b| b["label"] == "Open selected tool") && buttons.iter().any(|b| b["label"] == "Actions"), json!(buttons));
    chk(&mut checks, "remove_file_button_has_role_and_label", buttons.iter().any(|b| b["label"] == "Remove dropped file"), json!(buttons));
    chk(&mut checks, "every_interactive_node_has_label", missing.is_empty(), json!(missing));
    chk(&mut checks, "appkit_buttons_present_and_pressable", btn.len() >= 3 && btn.iter().all(|b| b["press_allowed"] == true), json!(btn.iter().map(|b| json!({"label": b["label"], "press_allowed": b["press_allowed"]})).collect::<Vec<_>>()));
    let sel_range = ak_field["selected_text_range"].clone();
    let nchars = ak_field["number_of_characters"].clone();
    chk(
        &mut checks,
        "appkit_field_selected_text_range_tracks_caret",
        sel_range == json!([3, 0]) && nchars == json!(3),
        json!({"accessibilitySelectedTextRange": sel_range, "accessibilityNumberOfCharacters": nchars, "caret_should_be": [3, 0],
               "why": "the adapter supports it only when the field publishes TextRun children and a text selection (a11y_synthetic_children); the bare kit Input does not, so a screen reader gets the value but no caret or per-character echo"}),
    );
    let _ = (st, ak_field);

    // Perform the VoiceOver "press" (accessibilityPerformPress) on the real AppKit nodes and observe the effect.
    let press_open = unsafe {
        let kids: Obj = msg_send![cv, accessibilityChildren];
        let root: Obj = msg_send![kids, objectAtIndex: 0usize];
        find_el(root, "Open selected tool", 0)
    };
    let mut press_result = json!({"found": !press_open.is_null()});
    if !press_open.is_null() {
        let ok: Bool = unsafe { msg_send![press_open, accessibilityPerformPress] };
        d.cx.background_executor().timer(Duration::from_millis(40)).await;
        d.draw();
        let msg = d.handle.update(d.cx, |bar, _, _| bar.footer_msg.clone()).unwrap_or(None);
        press_result = json!({"found": true, "performPress_returned": ok.as_bool(), "footer_msg_after": msg});
    }
    chk(&mut checks, "press_open_button_runs_selected_tool", press_result["footer_msg_after"].as_str().is_some_and(|m| m.starts_with("Opened ")), press_result);
    let press_rm = unsafe {
        let kids: Obj = msg_send![cv, accessibilityChildren];
        let root: Obj = msg_send![kids, objectAtIndex: 0usize];
        find_el(root, "Remove dropped file", 0)
    };
    let mut rm_result = json!({"found": !press_rm.is_null()});
    if !press_rm.is_null() {
        let before = d.handle.update(d.cx, |bar, _, _| bar.has_file()).unwrap_or(false);
        let ok: Bool = unsafe { msg_send![press_rm, accessibilityPerformPress] };
        d.cx.background_executor().timer(Duration::from_millis(40)).await;
        d.draw();
        let after = d.handle.update(d.cx, |bar, _, _| bar.has_file()).unwrap_or(true);
        rm_result = json!({"found": true, "performPress_returned": ok.as_bool(), "file_before": before, "file_after": after});
    }
    chk(&mut checks, "press_remove_file_button_removes_file", rm_result["file_before"] == true && rm_result["file_after"] == false, rm_result);

    json!({
        "test": "a11ydump",
        "window": d.window_json(),
        "how": "AccessKit woken by querying the content view's accessibilityChildren (what VoiceOver does), GPUI built its TreeUpdate via Window::draw, then read back from GPUI (debug_a11y_tree_json) and from AppKit (NSAccessibility selectors on the adapter's nodes)",
        "checks": checks,
        "pass_required": checks.iter().filter(|c| c["check"] != "appkit_field_selected_text_range_tracks_caret").all(|c| c["pass"] == true),
        "pass_all": checks.iter().all(|c| c["pass"] == true),
        "gpui_state_a": a,
        "gpui_state_b_rows_selection": sel_b,
        "gpui_state_c_devanagari": c,
        "appkit": appkit,
    })
}

pub async fn run(kind: &'static str, handle: WindowHandle<Bar>, cx: &mut AsyncApp) -> String {
    let Some(mut d) = Drv::new(handle, cx).await else {
        return "{\"error\":\"no ns view\"}".into();
    };
    // First frame so the input handler exists. Never presented, never shown.
    d.draw();
    d.settle().await;
    let v = match kind {
        "ime" => run_ime(&mut d).await,
        "keys" => run_keys(&mut d).await,
        "a11y" => run_a11y(&mut d).await,
        _ => run_shape(&mut d).await,
    };
    serde_json::to_string_pretty(&v).unwrap_or_else(|e| format!("{{\"error\":\"{e}\"}}"))
}
