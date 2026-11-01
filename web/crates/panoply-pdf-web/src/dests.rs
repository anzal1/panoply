//! Named destinations. Links and bookmarks often point at a name ("section.3.2") that a table in
//! the catalog resolves to a page. A result that does not carry that table (a split part, a
//! merge) turns the names into explicit destinations, so the links keep working when their page
//! is in the result and are dropped when it is not.

use std::collections::{HashMap, HashSet};

use crate::doc::Doc;
use crate::obj::{Dict, Obj};

/// All named destinations of a document, loaded on first use.
#[derive(Default)]
pub struct NameDests {
    map: Option<HashMap<Vec<u8>, Obj>>,
}

/// What a destination points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DestPage {
    /// The destination has no page of this document (a URI, a remote file, an unknown name).
    None,
    /// A page, by source object number.
    Page(u32),
    /// A name that could not be resolved.
    Unresolved,
}

impl NameDests {
    /// A resolver that loads the document's tables on first lookup.
    pub fn new() -> Self {
        Self::default()
    }

    fn table(&mut self, doc: &Doc) -> &HashMap<Vec<u8>, Obj> {
        self.map.get_or_insert_with(|| load_table(doc))
    }

    /// Look a name up. The result is the destination array.
    pub fn get(&mut self, doc: &Doc, name: &[u8]) -> Option<Obj> {
        self.table(doc).get(name).cloned()
    }

    /// Make the destination of a link or bookmark dictionary explicit, in place, and say which
    /// page it points at. Looks at `/Dest`, then at `/A` when that is a GoTo action.
    pub fn explicit(&mut self, doc: &Doc, d: &mut Dict) -> DestPage {
        if let Some(dest) = d.get("Dest").cloned() {
            let (res, new) = self.one(doc, &dest);
            if let Some(n) = new {
                d.set("Dest", n);
            }
            return res;
        }
        let Some(a) = d.get("A").cloned() else {
            return DestPage::None;
        };
        let Some(mut action) = doc.resolve_dict(&a) else {
            return DestPage::None;
        };
        if action.name("S") != Some(b"GoTo") {
            return DestPage::None;
        }
        let Some(target) = action.get("D").cloned() else {
            return DestPage::None;
        };
        let (res, new) = self.one(doc, &target);
        if let Some(n) = new {
            action.set("D", n);
            d.set("A", Obj::Dict(action));
        }
        res
    }

    fn one(&mut self, doc: &Doc, dest: &Obj) -> (DestPage, Option<Obj>) {
        match doc.resolve(dest) {
            Obj::Array(a) => (page_of(&a), None),
            Obj::Name(n) | Obj::Str(n) => match self.get(doc, &n) {
                Some(Obj::Array(a)) => {
                    let p = page_of(&a);
                    (p, Some(Obj::Array(a)))
                }
                _ => (DestPage::Unresolved, None),
            },
            _ => (DestPage::None, None),
        }
    }
}

fn page_of(a: &[Obj]) -> DestPage {
    match a.first() {
        Some(Obj::Ref(n, _)) => DestPage::Page(*n),
        // A remote destination holds a page number, not an object: not ours.
        _ => DestPage::None,
    }
}

fn load_table(doc: &Doc) -> HashMap<Vec<u8>, Obj> {
    let mut out: HashMap<Vec<u8>, Obj> = HashMap::new();
    let Ok(cat) = doc.catalog() else {
        return out;
    };
    let value = |o: &Obj| -> Option<Obj> {
        match doc.resolve(o) {
            Obj::Array(a) => Some(Obj::Array(a)),
            Obj::Dict(d) => d
                .get("D")
                .and_then(|x| doc.resolve_array(x))
                .map(Obj::Array),
            _ => None,
        }
    };
    // PDF 1.1 style: a dictionary in the catalog.
    if let Some(d) = cat.get("Dests").and_then(|d| doc.resolve_dict(d)) {
        for (k, v) in d.iter() {
            if let Some(v) = value(v) {
                out.insert(k.to_vec(), v);
            }
        }
    }
    // PDF 1.2 and later: a name tree under /Names.
    let root = cat
        .get("Names")
        .and_then(|n| doc.resolve_dict(n))
        .and_then(|n| n.get("Dests").cloned());
    if let Some(root) = root {
        let mut stack = vec![(root, 0usize)];
        let mut seen: HashSet<u32> = HashSet::new();
        while let Some((node, depth)) = stack.pop() {
            if depth > 32 || out.len() > 400_000 {
                break;
            }
            if let Some(n) = node.as_ref_num()
                && !seen.insert(n)
            {
                continue;
            }
            let Some(d) = doc.resolve_dict(&node) else {
                continue;
            };
            if let Some(names) = d.get("Names").and_then(|n| doc.resolve_array(n)) {
                for pair in names.chunks(2) {
                    if let [k, v] = pair
                        && let Obj::Str(k) = doc.resolve(k)
                        && let Some(v) = value(v)
                    {
                        out.insert(k, v);
                    }
                }
            }
            if let Some(kids) = d.get("Kids").and_then(|k| doc.resolve_array(k)) {
                for k in kids {
                    stack.push((k, depth + 1));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::Source;
    use crate::testutil::Builder;

    fn doc_with_names() -> Doc {
        let mut b = Builder::new();
        b.obj(1, "<< /Type /Catalog /Pages 2 0 R /Names << /Dests 6 0 R >> /Dests << /old [3 0 R /Fit] >> >>");
        b.obj(2, "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>");
        b.obj(3, "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] >>");
        b.obj(4, "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] >>");
        b.obj(5, "<< /D [4 0 R /XYZ 0 0 0] >>");
        b.obj(6, "<< /Kids [7 0 R] >>");
        b.obj(7, "<< /Names [(sec.2) 5 0 R (sec.1) [3 0 R /Fit]] >>");
        Doc::open(Source::from_bytes(b.build(1, ""))).unwrap()
    }

    #[test]
    fn names_resolve_through_tree_and_old_dict() {
        let doc = doc_with_names();
        let mut nd = NameDests::new();
        let mut link = Dict::new();
        link.set("Dest", Obj::Str(b"sec.2".to_vec()));
        assert_eq!(nd.explicit(&doc, &mut link), DestPage::Page(4));
        assert!(matches!(link.get("Dest"), Some(Obj::Array(_))));
        let mut old = Dict::new();
        old.set("Dest", Obj::name("old"));
        assert_eq!(nd.explicit(&doc, &mut old), DestPage::Page(3));
        let mut missing = Dict::new();
        missing.set("Dest", Obj::Str(b"nope".to_vec()));
        assert_eq!(nd.explicit(&doc, &mut missing), DestPage::Unresolved);
    }

    #[test]
    fn goto_actions_and_uris() {
        let doc = doc_with_names();
        let mut nd = NameDests::new();
        let mut goto = Dict::new();
        let mut act = Dict::new();
        act.set("S", Obj::name("GoTo"));
        act.set("D", Obj::Str(b"sec.1".to_vec()));
        goto.set("A", Obj::Dict(act));
        assert_eq!(nd.explicit(&doc, &mut goto), DestPage::Page(3));
        let mut uri = Dict::new();
        let mut a = Dict::new();
        a.set("S", Obj::name("URI"));
        uri.set("A", Obj::Dict(a));
        assert_eq!(nd.explicit(&doc, &mut uri), DestPage::None);
    }
}
