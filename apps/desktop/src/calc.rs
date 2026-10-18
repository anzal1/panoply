//! The bar's calculator: sums and unit conversions typed straight into the command bar.
//!
//! Pure Rust, no network, no state. [`answer`] reads the whole query or declines. It declines
//! whenever any word is not math (a tool name, a file word, "under 50 kb"), so a query meant for a
//! tool or a file is never taken over. A query counts as math only when it holds a computation: an
//! operator, a function, a percent or a conversion. A bare number or a bare constant is left to
//! search.
//!
//! Numbers are `f64`, rounded to 12 significant digits on the way out so `0.1 + 0.2` reads `0.3`.
//! Work per keystroke is one lex and one recursive descent over a short string: microseconds.

/// What the bar shows and copies for a calculation.
#[derive(Debug, Clone, PartialEq)]
pub struct Answer {
    /// The answer as drawn: "7,200", "3.106855961 mi".
    pub shown: String,
    /// The answer as copied: no digit grouping, so it pastes into a sheet or a shell.
    pub copy: String,
    /// What was asked, tidied: "15% of 2400", "5 km to mi".
    pub asked: String,
}

/// The answer to a typed query, or `None` when the query is not a calculation.
pub fn answer(query: &str) -> Option<Answer> {
    let q = query.trim().trim_start_matches('=').trim();
    if q.is_empty() || q.len() > 200 || looks_like_a_date_or_number_id(q) {
        return None;
    }
    let asked = q.to_string();
    let stripped = strip_thousands(q);
    let q = stripped.as_str();
    if let Some(v) = evaluate(q) {
        return Some(Answer {
            shown: group(&fmt(v, 10)),
            copy: fmt(v, 12),
            asked,
        });
    }
    convert(q)
}

/// Dates, year-months, month-days and phone numbers are not subtractions or fractions:
/// "2024-10-17", "3/4/2024", "2024-10", "10-17", "+1-555-1234", "555-1234". With spaces around the
/// operator it is plainly a sum ("2024 - 10"), and `10/4` and `5-3` stay math.
fn looks_like_a_date_or_number_id(q: &str) -> bool {
    if q.chars().any(char::is_whitespace) {
        return false;
    }
    let q = q.trim_start_matches('+');
    let groups: Vec<&str> = q.split(['-', '/']).collect();
    if groups.len() < 2
        || groups
            .iter()
            .any(|g| g.is_empty() || !g.chars().all(|c| c.is_ascii_digit()))
    {
        return false;
    }
    let month_day = groups.len() == 2
        && groups.iter().all(|g| g.len() == 2)
        && groups[0]
            .parse::<u32>()
            .is_ok_and(|m| (1..=12).contains(&m))
        && groups[1]
            .parse::<u32>()
            .is_ok_and(|d| (1..=31).contains(&d));
    groups.len() >= 3
        || groups[0].len() == 4
        || month_day
        || (q.contains('-') && groups[0].len() == 3 && groups[1].len() == 4)
}

/// "1,234,567" to "1234567": a comma between digits is a thousands separator when exactly three
/// digits follow it and one to three digits come before it. "1,5" and "1.5,000" are left alone, so
/// they fail to read and the bar says nothing.
fn strip_thousands(q: &str) -> String {
    let c: Vec<char> = q.chars().collect();
    let mut out = String::with_capacity(q.len());
    for (i, &ch) in c.iter().enumerate() {
        if ch == ',' && i > 0 && c[i - 1].is_ascii_digit() {
            let run = c[..i]
                .iter()
                .rev()
                .take_while(|d| d.is_ascii_digit())
                .count();
            let before = i.checked_sub(run + 1).map(|k| c[k]);
            let three = c
                .get(i + 1..i + 4)
                .is_some_and(|g| g.iter().all(char::is_ascii_digit))
                && !c.get(i + 4).is_some_and(char::is_ascii_digit);
            if (1..=3).contains(&run) && three && before != Some('.') {
                continue;
            }
        }
        out.push(ch);
    }
    out
}

// ---- math ----

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    Ident(String),
    Op(char),
    Open,
    Close,
}

fn lex(q: &str) -> Option<Vec<Tok>> {
    let c: Vec<char> = q.to_lowercase().chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        match ch {
            w if w.is_whitespace() => i += 1,
            '0'..='9' | '.' => {
                let start = i;
                let mut dots = 0;
                while i < c.len() && (c[i].is_ascii_digit() || c[i] == '.') {
                    dots += usize::from(c[i] == '.');
                    i += 1;
                }
                // An exponent is "e" then digits, or "e", a sign and digits: 1.5e3, 2e-4.
                if i < c.len() && c[i] == 'e' {
                    let sign = usize::from(matches!(c.get(i + 1), Some('+' | '-')));
                    if c.get(i + 1 + sign).is_some_and(char::is_ascii_digit) {
                        i += 1 + sign;
                        while i < c.len() && c[i].is_ascii_digit() {
                            i += 1;
                        }
                    }
                }
                if dots > 1 {
                    return None;
                }
                let text: String = c[start..i].iter().collect();
                out.push(Tok::Num(text.parse().ok()?));
            }
            '+' | '/' | '%' | '^' => {
                out.push(Tok::Op(ch));
                i += 1;
            }
            '-' | '\u{2212}' => {
                out.push(Tok::Op('-'));
                i += 1;
            }
            '\u{00D7}' => {
                out.push(Tok::Op('*'));
                i += 1;
            }
            '\u{00F7}' => {
                out.push(Tok::Op('/'));
                i += 1;
            }
            '*' => {
                if c.get(i + 1) == Some(&'*') {
                    out.push(Tok::Op('^'));
                    i += 2;
                } else {
                    out.push(Tok::Op('*'));
                    i += 1;
                }
            }
            '(' => {
                out.push(Tok::Open);
                i += 1;
            }
            ')' => {
                out.push(Tok::Close);
                i += 1;
            }
            '\u{03C0}' => {
                out.push(Tok::Ident("pi".into()));
                i += 1;
            }
            'a'..='z' => {
                let start = i;
                while i < c.len() && (c[i].is_ascii_lowercase() || c[i].is_ascii_digit()) {
                    i += 1;
                }
                out.push(Tok::Ident(c[start..i].iter().collect()));
            }
            _ => return None,
        }
    }
    Some(out)
}

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
    /// Operators, functions and percents seen. Zero means there was nothing to calculate.
    work: usize,
}

/// Evaluate a whole query as one expression.
fn evaluate(q: &str) -> Option<f64> {
    let toks = lex(q)?;
    if toks.is_empty() {
        return None;
    }
    let mut p = Parser {
        toks,
        pos: 0,
        work: 0,
    };
    let (v, _) = p.expr()?;
    (p.pos == p.toks.len() && p.work > 0 && v.is_finite()).then_some(v)
}

fn is_word(t: Option<&Tok>, w: &str) -> bool {
    matches!(t, Some(Tok::Ident(s)) if s == w)
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    /// Sum and difference. `a + b%` and `a - b%` take the percent of `a`.
    fn expr(&mut self) -> Option<(f64, bool)> {
        let (mut v, mut pct) = self.term()?;
        while let Some(Tok::Op(op @ ('+' | '-'))) = self.peek().cloned() {
            self.pos += 1;
            let (mut r, r_pct) = self.term()?;
            self.work += 1;
            if r_pct {
                r *= v;
            }
            v = if op == '+' { v + r } else { v - r };
            pct = false;
        }
        Some((v, pct))
    }

    /// Product, quotient, remainder and implicit multiplication (`2(3+4)`, `2pi`).
    fn term(&mut self) -> Option<(f64, bool)> {
        let (mut v, mut pct) = self.unary()?;
        loop {
            match self.peek().cloned() {
                Some(Tok::Op(op @ ('*' | '/' | '%'))) => {
                    self.pos += 1;
                    let (r, _) = self.unary()?;
                    self.work += 1;
                    v = match op {
                        '*' => v * r,
                        _ if r == 0.0 => return None,
                        '/' => v / r,
                        _ => v % r,
                    };
                }
                Some(Tok::Ident(w)) if w == "mod" => {
                    self.pos += 1;
                    let (r, _) = self.unary()?;
                    self.work += 1;
                    if r == 0.0 {
                        return None;
                    }
                    v %= r;
                }
                Some(Tok::Open) => {
                    let (r, _) = self.unary()?;
                    self.work += 1;
                    v *= r;
                }
                // No implicit multiplication into a bare `e`: "2e" is a number being typed
                // ("2e3"), not 2 times Euler's number. "2*e" still works.
                Some(Tok::Ident(w)) if w != "of" && w != "e" => {
                    let (r, _) = self.unary()?;
                    self.work += 1;
                    v *= r;
                }
                _ => break,
            }
            pct = false;
        }
        Some((v, pct))
    }

    fn unary(&mut self) -> Option<(f64, bool)> {
        match self.peek() {
            Some(Tok::Op('-')) => {
                self.pos += 1;
                let (v, _) = self.unary()?;
                Some((-v, false))
            }
            Some(Tok::Op('+')) => {
                self.pos += 1;
                self.unary()
            }
            _ => self.power(),
        }
    }

    /// `^` is right to left and binds tighter than a minus sign: `-2^2` is `-4`.
    fn power(&mut self) -> Option<(f64, bool)> {
        let (b, pct) = self.postfix()?;
        if matches!(self.peek(), Some(Tok::Op('^'))) {
            self.pos += 1;
            let (e, _) = self.unary()?;
            self.work += 1;
            let v = b.powf(e);
            return v.is_finite().then_some((v, false));
        }
        Some((b, pct))
    }

    /// `15%`, and `15% of 2400`. A `%` before a number or a bracket is a remainder, not a percent.
    fn postfix(&mut self) -> Option<(f64, bool)> {
        let mut v = self.atom()?;
        let mut pct = false;
        while matches!(self.peek(), Some(Tok::Op('%'))) {
            let after = self.toks.get(self.pos + 1);
            let percent = match after {
                None | Some(Tok::Close | Tok::Op(_)) => true,
                Some(t) => is_word(Some(t), "of"),
            };
            if !percent {
                break;
            }
            self.pos += 1;
            v /= 100.0;
            pct = true;
            if is_word(self.peek(), "of") {
                self.pos += 1;
                let (r, _) = self.unary()?;
                self.work += 1;
                v *= r;
                pct = false;
            }
        }
        Some((v, pct))
    }

    fn atom(&mut self) -> Option<f64> {
        let tok = self.peek()?.clone();
        self.pos += 1;
        match tok {
            Tok::Num(n) => Some(n),
            Tok::Open => {
                let (v, _) = self.expr()?;
                matches!(self.peek(), Some(Tok::Close)).then(|| self.pos += 1)?;
                Some(v)
            }
            Tok::Ident(name) => {
                if matches!(self.peek(), Some(Tok::Open)) {
                    self.pos += 1;
                    let (x, _) = self.expr()?;
                    matches!(self.peek(), Some(Tok::Close)).then(|| self.pos += 1)?;
                    self.work += 1;
                    return function(&name, x);
                }
                match name.as_str() {
                    "pi" => Some(std::f64::consts::PI),
                    "e" => Some(std::f64::consts::E),
                    "tau" => Some(std::f64::consts::TAU),
                    _ => None,
                }
            }
            Tok::Op(_) | Tok::Close => None,
        }
    }
}

fn function(name: &str, x: f64) -> Option<f64> {
    let v = match name {
        "sqrt" if x >= 0.0 => x.sqrt(),
        "cbrt" => x.cbrt(),
        "abs" => x.abs(),
        "sin" => x.sin(),
        "cos" => x.cos(),
        "tan" => x.tan(),
        "asin" => x.asin(),
        "acos" => x.acos(),
        "atan" => x.atan(),
        "ln" if x > 0.0 => x.ln(),
        "log" | "log10" if x > 0.0 => x.log10(),
        "log2" if x > 0.0 => x.log2(),
        "exp" => x.exp(),
        "floor" => x.floor(),
        "ceil" => x.ceil(),
        "round" => x.round(),
        _ => return None,
    };
    v.is_finite().then_some(v)
}

// ---- number text ----

/// `v` to `sig` significant digits as plain text. No exponent between 1e-6 and 1e15.
fn fmt(v: f64, sig: usize) -> String {
    let rounded: f64 = format!("{:.*e}", sig - 1, v).parse().unwrap_or(v);
    if rounded == 0.0 {
        return "0".into();
    }
    let a = rounded.abs();
    if (1e-6..1e15).contains(&a) {
        format!("{rounded}")
    } else {
        format!("{rounded:e}")
    }
}

/// Group the whole part in threes: "7200" to "7,200". Leaves exponent forms alone.
fn group(plain: &str) -> String {
    if plain.contains('e') {
        return plain.to_string();
    }
    let (sign, rest) = plain.strip_prefix('-').map_or(("", plain), |r| ("-", r));
    let (whole, frac) = rest.split_once('.').map_or((rest, ""), |(w, f)| (w, f));
    let mut out = String::from(sign);
    for (n, ch) in whole.chars().enumerate() {
        if n > 0 && (whole.len() - n).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    if !frac.is_empty() {
        out.push('.');
        out.push_str(frac);
    }
    out
}

// ---- units ----

#[derive(Clone, Copy, PartialEq, Eq)]
enum Dim {
    Length,
    Mass,
    Volume,
    Time,
    Data,
    Temp,
    Speed,
    Area,
}

struct Unit {
    names: &'static [&'static str],
    label: &'static str,
    dim: Dim,
    /// Size of one unit in the base unit of its dimension. Unused for temperature.
    per: f64,
}

const fn u(names: &'static [&'static str], label: &'static str, dim: Dim, per: f64) -> Unit {
    Unit {
        names,
        label,
        dim,
        per,
    }
}

/// Bytes, case blind and decimal, as the rest of the app reads sizes (`units::parse_size`), so `Mb`
/// is megabytes. Megabits are spelled `mbit`. Rates (`kbps`, `mb/s`) are not units here.
/// Lengths in metres, masses in kilograms, volumes in litres, times in seconds, speeds in metres
/// per second, areas in square metres.
const UNITS: &[Unit] = &[
    u(
        &[
            "mm",
            "millimeter",
            "millimeters",
            "millimetre",
            "millimetres",
        ],
        "mm",
        Dim::Length,
        0.001,
    ),
    u(
        &[
            "cm",
            "centimeter",
            "centimeters",
            "centimetre",
            "centimetres",
        ],
        "cm",
        Dim::Length,
        0.01,
    ),
    u(
        &["m", "meter", "meters", "metre", "metres"],
        "m",
        Dim::Length,
        1.0,
    ),
    u(
        &["km", "kilometer", "kilometers", "kilometre", "kilometres"],
        "km",
        Dim::Length,
        1000.0,
    ),
    u(&["in", "inch", "inches"], "in", Dim::Length, 0.0254),
    u(&["ft", "foot", "feet"], "ft", Dim::Length, 0.3048),
    u(&["yd", "yard", "yards"], "yd", Dim::Length, 0.9144),
    u(&["mi", "mile", "miles"], "mi", Dim::Length, 1609.344),
    u(&["nmi"], "nmi", Dim::Length, 1852.0),
    u(&["mg", "milligram", "milligrams"], "mg", Dim::Mass, 1e-6),
    u(&["g", "gram", "grams"], "g", Dim::Mass, 0.001),
    u(
        &["kg", "kilo", "kilos", "kilogram", "kilograms"],
        "kg",
        Dim::Mass,
        1.0,
    ),
    u(&["t", "tonne", "tonnes"], "t", Dim::Mass, 1000.0),
    u(
        &["oz", "ounce", "ounces"],
        "oz",
        Dim::Mass,
        0.028_349_523_125,
    ),
    u(
        &["lb", "lbs", "pound", "pounds"],
        "lb",
        Dim::Mass,
        0.453_592_37,
    ),
    u(&["st", "stone"], "st", Dim::Mass, 6.350_293_18),
    u(
        &[
            "ml",
            "milliliter",
            "milliliters",
            "millilitre",
            "millilitres",
        ],
        "mL",
        Dim::Volume,
        0.001,
    ),
    u(
        &["l", "liter", "liters", "litre", "litres"],
        "L",
        Dim::Volume,
        1.0,
    ),
    u(
        &["tsp", "teaspoon", "teaspoons"],
        "tsp",
        Dim::Volume,
        0.004_928_921_593_75,
    ),
    u(
        &["tbsp", "tablespoon", "tablespoons"],
        "tbsp",
        Dim::Volume,
        0.014_786_764_781_25,
    ),
    u(&["floz"], "fl oz", Dim::Volume, 0.029_573_529_562_5),
    u(&["cup", "cups"], "cup", Dim::Volume, 0.236_588_236_5),
    u(&["pt", "pint", "pints"], "pt", Dim::Volume, 0.473_176_473),
    u(&["qt", "quart", "quarts"], "qt", Dim::Volume, 0.946_352_946),
    u(
        &["gal", "gallon", "gallons"],
        "gal",
        Dim::Volume,
        3.785_411_784,
    ),
    u(
        &["ms", "millisecond", "milliseconds"],
        "ms",
        Dim::Time,
        0.001,
    ),
    u(
        &["s", "sec", "secs", "second", "seconds"],
        "s",
        Dim::Time,
        1.0,
    ),
    u(
        &["min", "mins", "minute", "minutes"],
        "min",
        Dim::Time,
        60.0,
    ),
    u(&["h", "hr", "hrs", "hour", "hours"], "h", Dim::Time, 3600.0),
    u(&["d", "day", "days"], "d", Dim::Time, 86_400.0),
    u(&["wk", "wks", "week", "weeks"], "wk", Dim::Time, 604_800.0),
    u(
        &["yr", "yrs", "year", "years"],
        "yr",
        Dim::Time,
        31_536_000.0,
    ),
    u(&["bit", "bits"], "bit", Dim::Data, 0.125),
    u(&["b", "byte", "bytes"], "B", Dim::Data, 1.0),
    u(&["kb", "kilobyte", "kilobytes"], "KB", Dim::Data, 1e3),
    u(&["mb", "megabyte", "megabytes"], "MB", Dim::Data, 1e6),
    u(&["gb", "gigabyte", "gigabytes"], "GB", Dim::Data, 1e9),
    u(&["tb", "terabyte", "terabytes"], "TB", Dim::Data, 1e12),
    u(&["pb", "petabyte", "petabytes"], "PB", Dim::Data, 1e15),
    u(&["kbit", "kbits"], "kbit", Dim::Data, 125.0),
    u(&["mbit", "mbits"], "Mbit", Dim::Data, 125_000.0),
    u(&["gbit", "gbits"], "Gbit", Dim::Data, 125_000_000.0),
    u(&["kib"], "KiB", Dim::Data, 1024.0),
    u(&["mib"], "MiB", Dim::Data, 1_048_576.0),
    u(&["gib"], "GiB", Dim::Data, 1_073_741_824.0),
    u(&["tib"], "TiB", Dim::Data, 1_099_511_627_776.0),
    u(&["c", "\u{00B0}c", "celsius"], "\u{00B0}C", Dim::Temp, 1.0),
    u(
        &["f", "\u{00B0}f", "fahrenheit"],
        "\u{00B0}F",
        Dim::Temp,
        1.0,
    ),
    u(&["k", "kelvin"], "K", Dim::Temp, 1.0),
    u(&["m/s", "mps"], "m/s", Dim::Speed, 1.0),
    u(&["km/h", "kmh", "kph"], "km/h", Dim::Speed, 1000.0 / 3600.0),
    u(&["mph"], "mph", Dim::Speed, 0.447_04),
    u(&["kn", "knot", "knots"], "kn", Dim::Speed, 1852.0 / 3600.0),
    u(&["sqm", "m2", "m\u{00B2}"], "m\u{00B2}", Dim::Area, 1.0),
    u(&["sqkm", "km2", "km\u{00B2}"], "km\u{00B2}", Dim::Area, 1e6),
    u(
        &["sqft", "ft2", "ft\u{00B2}"],
        "ft\u{00B2}",
        Dim::Area,
        0.092_903_04,
    ),
    u(
        &["sqmi", "mi2", "mi\u{00B2}"],
        "mi\u{00B2}",
        Dim::Area,
        2_589_988.110_336,
    ),
    u(&["ha", "hectare", "hectares"], "ha", Dim::Area, 10_000.0),
    u(&["acre", "acres"], "acre", Dim::Area, 4_046.856_422_4),
];

fn unit(word: &str) -> Option<&'static Unit> {
    UNITS.iter().find(|u| u.names.contains(&word))
}

/// "5km" to (5, "km"). The number is a plain decimal with an optional sign.
fn split_attached(word: &str) -> Option<(f64, &str)> {
    let digits_from = usize::from(word.starts_with(['-', '+']));
    let rest = &word[digits_from..];
    let end = rest
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(rest.len());
    if end == 0 {
        return None;
    }
    let n: f64 = word[..digits_from + end].parse().ok()?;
    Some((n, &word[digits_from + end..]))
}

fn plain_number(word: &str) -> Option<f64> {
    let (n, rest) = split_attached(word)?;
    rest.is_empty().then_some(n)
}

/// To degrees Celsius, or from them.
fn temp_to_c(v: f64, from: &Unit) -> f64 {
    match from.label {
        "\u{00B0}F" => (v - 32.0) * 5.0 / 9.0,
        "K" => v - 273.15,
        _ => v,
    }
}

fn temp_from_c(c: f64, to: &Unit) -> f64 {
    match to.label {
        "\u{00B0}F" => c * 9.0 / 5.0 + 32.0,
        "K" => c + 273.15,
        _ => c,
    }
}

/// `<number> <unit> in|to|as|into <unit>`: "5 km in miles", "72 f to c", "500kb to mb".
fn convert(q: &str) -> Option<Answer> {
    let lower = q.to_lowercase();
    let words: Vec<&str> = lower.split_whitespace().collect();
    // A rate is not a size: "5 mbps in kbps" must not be read as megabytes.
    const RATES: &[&str] = &[
        "kbps", "mbps", "gbps", "kb/s", "mb/s", "gb/s", "kbit/s", "mbit/s",
    ];
    if words.iter().any(|w| RATES.iter().any(|r| w.ends_with(r))) {
        return None;
    }
    for i in 1..words.len().saturating_sub(1) {
        if !matches!(words[i], "in" | "to" | "as" | "into") || words.len() - i != 2 {
            continue;
        }
        let (n, from_word) = match &words[..i] {
            [one] => match split_attached(one) {
                Some((n, w)) if !w.is_empty() => (n, w),
                _ => continue,
            },
            [num, w] => match plain_number(num) {
                Some(n) => (n, *w),
                None => continue,
            },
            _ => continue,
        };
        let (Some(from), Some(to)) = (unit(from_word), unit(words[i + 1])) else {
            continue;
        };
        if from.dim != to.dim {
            continue;
        }
        let v = if from.dim == Dim::Temp {
            temp_from_c(temp_to_c(n, from), to)
        } else {
            n * from.per / to.per
        };
        if !v.is_finite() {
            continue;
        }
        let asked = format!("{} {} to {}", fmt(n, 12), from.label, to.label);
        return Some(Answer {
            shown: format!("{} {}", group(&fmt(v, 10)), to.label),
            copy: fmt(v, 12),
            asked,
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Query to the copied text, or None when the bar must leave the query alone.
    fn copy(q: &str) -> Option<String> {
        answer(q).map(|a| a.copy)
    }

    #[test]
    fn table_of_answers() {
        let cases: &[(&str, &str)] = &[
            // Arithmetic and precedence.
            ("2+2*3", "8"),
            ("2 + 2", "4"),
            ("(2+2)*3", "12"),
            ("10/4", "2.5"),
            ("10 - 3 - 2", "5"),
            ("2^10", "1024"),
            ("2^3^2", "512"),
            ("-2^2", "-4"),
            ("2**8", "256"),
            ("7 mod 3", "1"),
            ("10 % 3", "1"),
            ("2\u{00D7}3", "6"),
            ("9\u{00F7}3", "3"),
            ("=1+1", "2"),
            // Float noise is hidden.
            ("0.1+0.2", "0.3"),
            ("1/3", "0.333333333333"),
            // Percent.
            ("15% of 2400", "360"),
            ("2400 + 15%", "2760"),
            ("2400 - 15%", "2040"),
            ("100 + 10%", "110"),
            ("50% of 80 + 1", "41"),
            // Functions and constants.
            ("sqrt(2)", "1.41421356237"),
            ("sqrt(16)", "4"),
            ("abs(-5)", "5"),
            ("2*pi", "6.28318530718"),
            ("floor(2.7)+ceil(2.1)", "5"),
            ("log(1000)", "3"),
            ("log2(8)", "3"),
            ("ln(e)", "1"),
            ("2(3+4)", "14"),
            // Scientific notation in the input and the output.
            ("1.5e3/7", "214.285714286"),
            ("2e-3*2", "0.004"),
            ("10^20", "1e20"),
            // Conversions.
            ("5 km in miles", "3.10685596119"),
            ("72 f to c", "22.2222222222"),
            ("2 GB in MB", "2000"),
            ("500 kb in mb", "0.5"),
            ("500kb to mb", "0.5"),
            ("0 c to f", "32"),
            ("-40 f to c", "-40"),
            ("100 c in k", "373.15"),
            ("1 gib to mib", "1024"),
            ("10 in to cm", "25.4"),
            ("1 mile as km", "1.609344"),
            ("60 mph in km/h", "96.56064"),
            ("2 hours to minutes", "120"),
            ("1 kg in lb", "2.20462262185"),
        ];
        for (q, want) in cases {
            assert_eq!(copy(q).as_deref(), Some(*want), "query: {q}");
        }
    }

    #[test]
    fn queries_meant_for_tools_or_files_are_left_alone() {
        let cases = [
            "under 50 kb",
            "make this under 5 mb",
            "merge pdf",
            "compress pdf",
            "heic to jpg",
            "pdf to word",
            "convert 5 mb to kb",
            "resize to 800px",
            "loudness -16",
            "passport photo india",
            "to jpg",
            "50 kb",
            "5 mb to",
            "invoice 2024",
            "2024",
            "-16",
            "pi",
            "e",
            "(5)",
            "15%",
            "2 pdf",
            "sqrt",
            "report.pdf",
            "1080p",
            "5 kg in km",
            "5 widgets in boxes",
        ];
        for q in cases {
            assert_eq!(answer(q), None, "query: {q}");
        }
    }

    #[test]
    fn dates_and_ids_are_not_sums() {
        for q in ["2024-10-17", "3/4/2024", "555-1234", "1-2-3", "2024/10/17"] {
            assert_eq!(answer(q), None, "query: {q}");
        }
        // With spaces it is plainly a sum.
        assert_eq!(copy("2024 - 10 - 17").as_deref(), Some("1997"));
    }

    #[test]
    fn broken_or_impossible_math_gives_no_answer() {
        for q in [
            "1/0", "5 mod 0", "sqrt(-1)", "ln(0)", "2+", "(2+3", "2+3)", "1.2.3+1", "1e999*2",
            "10^999",
        ] {
            assert_eq!(answer(q), None, "query: {q}");
        }
    }

    #[test]
    fn the_bar_text_is_grouped_and_the_copy_is_not() {
        let a = answer("15% of 48000").unwrap();
        assert_eq!(a.shown, "7,200");
        assert_eq!(a.copy, "7200");
        assert_eq!(a.asked, "15% of 48000");
        let a = answer("2 GB in kb").unwrap();
        assert_eq!(a.shown, "2,000,000 KB");
        assert_eq!(a.copy, "2000000");
        assert_eq!(a.asked, "2 GB to KB");
        assert_eq!(answer("1-5000").unwrap().shown, "-4,999");
    }

    #[test]
    fn conversions_copy_the_bare_number_and_show_the_unit() {
        let a = answer("5 km in miles").unwrap();
        assert_eq!(a.copy, "3.10685596119");
        assert_eq!(a.shown, "3.106855961 mi");
        assert_eq!(copy("72 f to c").as_deref(), Some("22.2222222222"));
    }

    #[test]
    fn year_months_month_days_and_phone_numbers_are_not_sums() {
        for q in [
            "2024-10",
            "2024/10",
            "10-17",
            "10/17",
            "12-31",
            "+1-555-1234",
            "+91-98765-43210",
            "1999-12",
        ] {
            assert_eq!(answer(q), None, "query: {q}");
        }
        // Still math: short ones that are not a month and a day, and anything with spaces.
        assert_eq!(copy("10-5").as_deref(), Some("5"));
        assert_eq!(copy("20-15").as_deref(), Some("5"));
        assert_eq!(copy("10/4").as_deref(), Some("2.5"));
        assert_eq!(copy("2024 - 10").as_deref(), Some("2014"));
        assert_eq!(copy("+5-3").as_deref(), Some("2"));
    }

    #[test]
    fn a_number_being_typed_is_not_times_e() {
        assert_eq!(answer("2e"), None);
        assert_eq!(answer("2 e"), None);
        assert_eq!(copy("2e3+1").as_deref(), Some("2001"));
        assert_eq!(copy("2*e").as_deref(), Some("5.43656365692"));
        assert_eq!(copy("2pi").as_deref(), Some("6.28318530718"));
    }

    #[test]
    fn megabits_are_spelled_out_and_rates_are_not_sizes() {
        assert_eq!(copy("1 mbit in kb").as_deref(), Some("125"));
        assert_eq!(copy("8 kbit to bytes").as_deref(), Some("1000"));
        assert_eq!(copy("1 gb in gbit").as_deref(), Some("8"));
        // Mb is megabytes, as everywhere in the app.
        assert_eq!(copy("1 Mb in kb").as_deref(), Some("1000"));
        for q in [
            "5 mbps in kbps",
            "100 kbps to mbps",
            "10 mb/s in kb/s",
            "5 gbps to mb",
        ] {
            assert_eq!(answer(q), None, "query: {q}");
        }
    }

    #[test]
    fn thousands_separators_and_non_breaking_spaces_read() {
        assert_eq!(copy("1,000+5").as_deref(), Some("1005"));
        assert_eq!(copy("1,234,567 / 1000").as_deref(), Some("1234.567"));
        assert_eq!(copy("12,345.5 * 2").as_deref(), Some("24691"));
        assert_eq!(copy("1,000 kb in mb").as_deref(), Some("1"));
        assert_eq!(copy("2\u{00A0}+\u{00A0}2").as_deref(), Some("4"));
        assert_eq!(copy("500\u{00A0}kb in mb").as_deref(), Some("0.5"));
        assert_eq!(copy("500 kb\u{202F}in\u{202F}mb").as_deref(), Some("0.5"));
        // Not thousands: the bar says nothing.
        for q in ["1,5+1", "1,2345+1", "1234,567+1", "1.5,000+1", "1,0+2"] {
            assert_eq!(answer(q), None, "query: {q}");
        }
        // The question is shown as typed.
        assert_eq!(answer("1,000+5").unwrap().asked, "1,000+5");
    }

    #[test]
    fn a_keystroke_costs_microseconds() {
        let start = std::time::Instant::now();
        for _ in 0..1000 {
            std::hint::black_box(answer("(1.5e3 + 2^10) / sqrt(2) * 15% of 2400"));
            std::hint::black_box(answer("500 kb in mb"));
            std::hint::black_box(answer("make this under 50 kb"));
        }
        let per_call = start.elapsed().as_secs_f64() * 1000.0 / 3000.0;
        assert!(per_call < 1.0, "{per_call} ms per call");
    }
}
