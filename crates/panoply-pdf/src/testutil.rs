//! Builders for small test PDFs with a classic cross-reference table. Test code only.

/// A PDF assembled by hand from numbered objects.
#[derive(Default)]
pub struct Builder {
    objs: Vec<(u32, Vec<u8>)>,
}

impl Builder {
    /// An empty builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add `num 0 obj <body> endobj`.
    pub fn obj(&mut self, num: u32, body: &str) -> &mut Self {
        self.objs.push((num, body.as_bytes().to_vec()));
        self
    }

    /// Add a stream object. `dict` is the dictionary text without `<<`, `>>` and `/Length`.
    pub fn stream(&mut self, num: u32, dict: &str, data: &[u8]) -> &mut Self {
        let mut v = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
        v.extend_from_slice(data);
        v.extend_from_slice(b"\nendstream");
        self.objs.push((num, v));
        self
    }

    /// Write the file with `root` as the catalog and optional trailer extras.
    pub fn build(&self, root: u32, trailer_extra: &str) -> Vec<u8> {
        let mut out = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
        let max = self.objs.iter().map(|o| o.0).max().unwrap_or(0);
        let mut offs = vec![None; max as usize + 1];
        for (n, body) in &self.objs {
            offs[*n as usize] = Some(out.len());
            out.extend_from_slice(format!("{n} 0 obj\n").as_bytes());
            out.extend_from_slice(body);
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref = out.len();
        out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", max + 1).as_bytes());
        for o in offs.iter().skip(1) {
            match o {
                Some(o) => out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes()),
                None => out.extend_from_slice(b"0000000000 00000 f \n"),
            }
        }
        out.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root {root} 0 R {trailer_extra} >>\nstartxref\n{xref}\n%%EOF\n",
                max + 1
            )
            .as_bytes(),
        );
        out
    }
}

/// One page of [`text_pdf`].
pub struct TextPage {
    /// Width in points.
    pub w: u32,
    /// Height in points.
    pub h: u32,
    /// `/Rotate` of the page.
    pub rotate: u32,
    /// The line of text drawn on it.
    pub text: String,
}

impl TextPage {
    /// A page with a label.
    pub fn new(w: u32, h: u32, rotate: u32, text: &str) -> Self {
        TextPage {
            w,
            h,
            rotate,
            text: text.into(),
        }
    }
}

/// A PDF with one Helvetica text line per page. Objects: 1 catalog, 2 pages, 3 font,
/// then page and content per page. Sizes and rotation are on the pages (not inherited).
pub fn text_pdf(pages: &[TextPage]) -> Vec<u8> {
    let mut b = Builder::new();
    b.obj(1, "<< /Type /Catalog /Pages 2 0 R >>");
    let kids: Vec<String> = (0..pages.len())
        .map(|i| format!("{} 0 R", 4 + 2 * i))
        .collect();
    b.obj(
        2,
        &format!(
            "<< /Type /Pages /Kids [{}] /Count {} >>",
            kids.join(" "),
            pages.len()
        ),
    );
    b.obj(3, "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>");
    for (i, p) in pages.iter().enumerate() {
        let (pg, ct) = (4 + 2 * i as u32, 5 + 2 * i as u32);
        b.obj(
            pg,
            &format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Rotate {} /Contents {ct} 0 R /Resources << /Font << /F1 3 0 R >> >> >>",
                p.w, p.h, p.rotate
            ),
        );
        let content = format!(
            "BT /F1 24 Tf 40 {} Td ({}) Tj ET",
            p.h.saturating_sub(60),
            p.text
        );
        b.stream(ct, "", content.as_bytes());
    }
    b.build(1, "")
}

/// The smallest interesting file: two pages, the first inherits size and rotation.
pub fn tiny_pdf() -> Vec<u8> {
    let mut b = Builder::new();
    b.obj(1, "<< /Type /Catalog /Pages 2 0 R >>");
    b.obj(
        2,
        "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 300 400] /Rotate 90 >>",
    );
    b.obj(3, "<< /Type /Page /Parent 2 0 R /Contents 5 0 R >>");
    b.obj(
        4,
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 200] /Rotate 0 /Contents 5 0 R >>",
    );
    b.stream(5, "", b"q Q ");
    b.build(1, "")
}
