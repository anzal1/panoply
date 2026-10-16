//! The pure part: text to a QR code as an SVG or PNG.

use qrcode::{Color, EcLevel, QrCode, types::QrError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub fn hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }

    fn luminance(self) -> f32 {
        let f = |c: u8| {
            let c = c as f32 / 255.0;
            if c <= 0.03928 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * f(self.0) + 0.7152 * f(self.1) + 0.0722 * f(self.2)
    }
}

/// "#1a2b3c", "1a2b3c" or "#fff".
pub fn parse_colour(s: &str) -> Result<Rgb, String> {
    let h = s.trim().trim_start_matches('#');
    let bad = || format!("\"{s}\" is not a colour. Write it like #111111.");
    let d = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).map_err(|_| bad());
    if !h.is_ascii() {
        return Err(bad());
    }
    match h.len() {
        6 => Ok(Rgb(d(0)?, d(2)?, d(4)?)),
        3 => {
            let e = |i: usize| {
                u8::from_str_radix(&h[i..i + 1], 16)
                    .map(|v| v * 17)
                    .map_err(|_| bad())
            };
            Ok(Rgb(e(0)?, e(1)?, e(2)?))
        }
        _ => Err(bad()),
    }
}

/// Dark squares must stand well apart from the light ones or phones cannot read the code.
pub fn check_contrast(dark: Rgb, light: Rgb) -> Result<(), String> {
    let (a, b) = (dark.luminance(), light.luminance());
    let ratio = (a.max(b) + 0.05) / (a.min(b) + 0.05);
    if a >= b {
        return Err("The squares are lighter than the background. Phones read dark squares on a light background, so swap the colours.".into());
    }
    if ratio < 3.0 {
        return Err("The squares and the background are too close in colour for a phone to read. Make the squares much darker or the background much lighter.".into());
    }
    Ok(())
}

pub fn level(s: &str) -> EcLevel {
    match s {
        "L" => EcLevel::L,
        "Q" => EcLevel::Q,
        "H" => EcLevel::H,
        _ => EcLevel::M,
    }
}

pub struct Matrix {
    pub width: usize,
    pub dark: Vec<bool>,
}

pub fn encode(text: &str, ec: EcLevel) -> Result<Matrix, String> {
    let code = QrCode::with_error_correction_level(text.as_bytes(), ec).map_err(|e| match e {
        QrError::DataTooLong => "That is too much text for one QR code (about 2,900 characters at most, less with a higher damage tolerance). Shorten it, or lower the damage tolerance.".to_string(),
        other => format!("Could not make a code: {other}"),
    })?;
    Ok(Matrix {
        width: code.width(),
        dark: code
            .to_colors()
            .into_iter()
            .map(|c| c == Color::Dark)
            .collect(),
    })
}

pub fn svg(m: &Matrix, border: usize, dark: Rgb, light: Rgb) -> String {
    let size = m.width + border * 2;
    let mut path = String::new();
    for y in 0..m.width {
        // One rectangle per run of dark squares keeps the file small.
        let mut x = 0;
        while x < m.width {
            if m.dark[y * m.width + x] {
                let start = x;
                while x < m.width && m.dark[y * m.width + x] {
                    x += 1;
                }
                path.push_str(&format!(
                    "M{} {}h{}v1h-{}z",
                    start + border,
                    y + border,
                    x - start,
                    x - start
                ));
            } else {
                x += 1;
            }
        }
    }
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {size} {size}\" width=\"{px}\" height=\"{px}\" shape-rendering=\"crispEdges\" role=\"img\" aria-label=\"QR code\">\n<rect width=\"{size}\" height=\"{size}\" fill=\"{light}\"/>\n<path fill=\"{dark}\" d=\"{path}\"/>\n</svg>\n",
        px = size * 10,
        light = light.hex(),
        dark = dark.hex(),
    )
}

pub fn png(
    m: &Matrix,
    border: usize,
    scale: usize,
    dark: Rgb,
    light: Rgb,
) -> Result<Vec<u8>, String> {
    let side = (m.width + border * 2) * scale;
    let mut pixels = Vec::with_capacity(side * side);
    for py in 0..side {
        let qy = py / scale;
        for px in 0..side {
            let qx = px / scale;
            let is_dark = qx >= border
                && qy >= border
                && qx < border + m.width
                && qy < border + m.width
                && m.dark[(qy - border) * m.width + (qx - border)];
            pixels.push(u8::from(is_dark));
        }
    }
    // Two colours: an indexed PNG is a fraction of the size of an RGB one.
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, side as u32, side as u32);
        enc.set_color(png::ColorType::Indexed);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_palette(vec![light.0, light.1, light.2, dark.0, dark.1, dark.2]);
        let mut w = enc.write_header().map_err(|e| e.to_string())?;
        w.write_image_data(&pixels).map_err(|e| e.to_string())?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLACK: Rgb = Rgb(0, 0, 0);
    const WHITE: Rgb = Rgb(255, 255, 255);

    #[test]
    fn a_link_makes_a_version_two_or_smaller_code() {
        let m = encode("https://anzalabidi.dev", EcLevel::M).unwrap();
        // Version 2 is 25 squares wide, version 3 is 29.
        assert!(m.width == 25 || m.width == 29, "{}", m.width);
        assert_eq!(m.dark.len(), m.width * m.width);
        // The top-left finder pattern: a dark 7x7 ring with a dark centre.
        assert!(m.dark[0] && m.dark[6] && m.dark[3 * m.width + 3]);
    }

    #[test]
    fn the_png_is_a_real_png_of_the_right_size() {
        let m = encode("hello", EcLevel::L).unwrap();
        let bytes = png(&m, 4, 10, BLACK, WHITE).unwrap();
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
        let dec = png::Decoder::new(std::io::Cursor::new(&bytes));
        let mut r = dec.read_info().unwrap();
        let mut buf = vec![0; r.output_buffer_size().unwrap()];
        let info = r.next_frame(&mut buf).unwrap();
        let side = (m.width + 8) * 10;
        assert_eq!((info.width as usize, info.height as usize), (side, side));
        // The quiet border is light, and the first finder square (module 0,0) is dark.
        assert_eq!(buf[0], 0);
        let first_dark = (4 * 10) * side + 4 * 10;
        assert_eq!(buf[first_dark], 1);
    }

    #[test]
    fn the_svg_has_one_path_and_the_colours() {
        let m = encode("hello", EcLevel::Q).unwrap();
        let s = svg(&m, 4, Rgb(0x11, 0x22, 0x33), WHITE);
        assert!(s.starts_with("<svg "));
        assert_eq!(s.matches("<path").count(), 1);
        assert!(s.contains("fill=\"#112233\"") && s.contains("fill=\"#ffffff\""));
        assert!(s.contains(&format!("viewBox=\"0 0 {0} {0}\"", m.width + 8)));
    }

    #[test]
    fn higher_damage_tolerance_makes_a_bigger_code() {
        let text = "https://example.com/a/fairly/long/path?with=query&and=more";
        assert!(encode(text, EcLevel::H).unwrap().width > encode(text, EcLevel::L).unwrap().width);
    }

    #[test]
    fn too_much_text_is_explained() {
        let e = encode(&"x".repeat(4000), EcLevel::M).err().unwrap();
        assert!(e.contains("too much text"), "{e}");
    }

    #[test]
    fn colours_parse_and_bad_ones_say_so() {
        assert_eq!(parse_colour("#1a2b3c").unwrap(), Rgb(0x1a, 0x2b, 0x3c));
        assert_eq!(parse_colour("fff").unwrap(), WHITE);
        assert!(parse_colour("blue").unwrap_err().contains("not a colour"));
        assert!(parse_colour("#12345").is_err());
        assert!(parse_colour("#ééé").is_err());
    }

    #[test]
    fn unreadable_colour_pairs_are_refused() {
        assert!(check_contrast(BLACK, WHITE).is_ok());
        assert!(check_contrast(Rgb(0, 0x40, 0x90), Rgb(0xee, 0xf4, 0xff)).is_ok());
        assert!(check_contrast(WHITE, BLACK).unwrap_err().contains("swap"));
        assert!(
            check_contrast(Rgb(0x88, 0x88, 0x88), Rgb(0xaa, 0xaa, 0xaa))
                .unwrap_err()
                .contains("too close")
        );
    }
}
