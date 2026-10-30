//! Invoice PDF generated in Rust (spike S-02, path b). It draws the same model as the HTML page
//! (`crate::invoice`), so the totals are the money crate's; the layout follows design/invoice.html by hand.
//!
//! The layout is done once, into a display list of positioned, shaped text runs, hairlines and rectangles. Text is
//! shaped with rustybuzz (kerning, and tabular figures for amounts, as CSS `tabular-nums`). Two backends write the
//! list as PDF so the spike can compare the crates on equal terms: `printpdf` and `krilla`.
//!
//! US Letter, Geist (static TTFs, subset into the file). Long invoices break between rows: the table header
//! repeats on every page, a row is never split, the totals block stays whole, and each page has "Page n of N".

mod krilla_backend;
mod printpdf_backend;

use crate::invoice::{display_date, Invoice, Line};
use rustybuzz::{Feature, UnicodeBuffer};
use std::ops::Range;
use std::str::FromStr;
use std::sync::LazyLock;

/// Which crate writes the PDF.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Engine {
    Printpdf,
    Krilla,
}

/// Renders the invoice as a PDF file.
pub fn invoice_pdf(inv: &Invoice, engine: Engine) -> Vec<u8> {
    let pages = layout(inv);
    let title = format!("Invoice {}", inv.doc_number);
    match engine {
        Engine::Printpdf => printpdf_backend::write(&title, &pages),
        Engine::Krilla => krilla_backend::write(&title, &pages),
    }
}

pub(crate) const PAGE_W: f32 = 612.0; // US Letter, pt
pub(crate) const PAGE_H: f32 = 792.0;
const MARGIN: f32 = 42.0; // about 14.8 mm, as the print CSS
const CONTENT_W: f32 = PAGE_W - 2.0 * MARGIN;
/// The lowest point for content; below it are the page footer and the margin.
const BOTTOM: f32 = PAGE_H - MARGIN - 18.0;

// Design sizes in CSS px x 0.75.
const BODY: f32 = 9.75; // 13px
const SMALL: f32 = 9.0; // 12px
const META: f32 = 9.4; // 12.5px
const TH: f32 = 8.6; // 11.5px
const EYEBROW: f32 = 8.25; // 11px
const BIG: f32 = 19.5; // 26px
const BALANCE: f32 = 11.25; // 15px
const FOOTER: f32 = 7.5;

// Table columns.
const COL_ITEM_W: f32 = 92.0;
const COL_QTY_W: f32 = 40.0;
const COL_RATE_W: f32 = 70.0;
const COL_TAX_W: f32 = 34.0;
const COL_AMOUNT_W: f32 = 80.0;
const COL_DESC_W: f32 = CONTENT_W - COL_ITEM_W - COL_QTY_W - COL_RATE_W - COL_TAX_W - COL_AMOUNT_W;
const ROW_PAD: f32 = 6.0;
const ROW_LEADING: f32 = 13.0;

pub(crate) const FONT_FILES: [&[u8]; 4] = [
    include_bytes!("../../assets/vendor/fonts/static/Geist-Regular.ttf"),
    include_bytes!("../../assets/vendor/fonts/static/Geist-Medium.ttf"),
    include_bytes!("../../assets/vendor/fonts/static/Geist-SemiBold.ttf"),
    include_bytes!("../../assets/vendor/fonts/static/GeistMono-Regular.ttf"),
];

static FACES: LazyLock<[rustybuzz::Face<'static>; 4]> =
    LazyLock::new(|| FONT_FILES.map(|b| rustybuzz::Face::from_slice(b, 0).expect("vendored Geist TTF parses")));

/// The four faces, as indexes into `FONT_FILES`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Face {
    Regular = 0,
    Medium = 1,
    Semibold = 2,
    Mono = 3,
}

/// sRGB, 0..1. The print palette of styles/app.css (oklch converted).
pub(crate) type Rgb = [f32; 3];
const CONTENT: Rgb = [0.118, 0.100, 0.086];
const MUTED: Rgb = [0.384, 0.362, 0.345];
const HAIRLINE: Rgb = [0.852, 0.843, 0.831];
const PRIMARY: Rgb = [0.812, 0.272, 0.0];
const EMBER: Rgb = [0.86, 0.34, 0.10];
const WHITE: Rgb = [1.0, 1.0, 1.0];

/// One shaped glyph: its id, advance in em (letter spacing included) and the text it stands for.
#[derive(Clone, Debug)]
pub(crate) struct Glyph {
    pub id: u16,
    pub advance: f32,
    pub range: Range<usize>,
}

/// A display-list item. `y` values are from the top of the page; text is placed by its baseline.
#[derive(Clone, Debug)]
pub(crate) enum Draw {
    Text { face: Face, size: f32, x: f32, baseline: f32, text: String, glyphs: Vec<Glyph>, color: Rgb },
    Line { x1: f32, x2: f32, y: f32, color: Rgb },
    Rect { x: f32, y: f32, w: f32, h: f32, color: Rgb },
}

/// Shapes `text` in `face`; `tabular` turns on tabular figures, `tracking` adds letter spacing in em.
fn shape(face: Face, text: &str, tabular: bool, tracking: f32) -> Vec<Glyph> {
    let rb = &FACES[face as usize];
    let mut buf = UnicodeBuffer::new();
    buf.push_str(text);
    buf.guess_segment_properties();
    let features: Vec<Feature> =
        if tabular { vec![Feature::from_str("tnum").expect("valid feature")] } else { Vec::new() };
    let out = rustybuzz::shape(rb, &features, buf);
    let upem = rb.units_per_em() as f32;
    let infos = out.glyph_infos();
    let positions = out.glyph_positions();
    (0..out.len())
        .map(|i| {
            let start = infos[i].cluster as usize;
            let end = infos[i + 1..].iter().map(|g| g.cluster as usize).find(|&c| c != start).unwrap_or(text.len());
            Glyph {
                id: infos[i].glyph_id as u16,
                advance: positions[i].x_advance as f32 / upem + tracking,
                range: start..end,
            }
        })
        .collect()
}

fn width(glyphs: &[Glyph], size: f32) -> f32 {
    glyphs.iter().map(|g| g.advance).sum::<f32>() * size
}

/// Collects the display list page by page; `y` runs down from the top of the page.
struct Writer {
    pages: Vec<Vec<Draw>>,
    items: Vec<Draw>,
    y: f32,
}

#[derive(Clone, Copy)]
enum Align {
    Left,
    Right,
    Center,
}

#[derive(Clone, Copy)]
struct Style {
    face: Face,
    size: f32,
    color: Rgb,
    tabular: bool,
    tracking: f32,
}

const fn style(face: Face, size: f32, color: Rgb) -> Style {
    Style { face, size, color, tabular: false, tracking: 0.0 }
}

const fn num(face: Face, size: f32, color: Rgb) -> Style {
    Style { face, size, color, tabular: true, tracking: 0.0 }
}

impl Writer {
    /// Draws `text` with its left, right or center at `x`; returns its width.
    fn text(&mut self, s: Style, align: Align, x: f32, baseline: f32, text: &str) -> f32 {
        let glyphs = shape(s.face, text, s.tabular, s.tracking);
        let w = width(&glyphs, s.size);
        let x = match align {
            Align::Left => x,
            Align::Right => x - w,
            Align::Center => x - w / 2.0,
        };
        self.items.push(Draw::Text {
            face: s.face,
            size: s.size,
            x,
            baseline,
            text: text.to_string(),
            glyphs,
            color: s.color,
        });
        w
    }

    /// Uppercase label with 0.06em letter spacing, as `.eyebrow`.
    fn eyebrow(&mut self, align: Align, x: f32, baseline: f32, text: &str) {
        let s = Style { face: Face::Medium, size: EYEBROW, color: MUTED, tabular: false, tracking: 0.06 };
        self.text(s, align, x, baseline, &text.to_uppercase());
    }

    fn hline(&mut self, x1: f32, x2: f32, y: f32) {
        self.items.push(Draw::Line { x1, x2, y, color: HAIRLINE });
    }

    fn new_page(&mut self) {
        self.pages.push(std::mem::take(&mut self.items));
        self.y = MARGIN;
    }
}

/// Greedy word wrap of `text` to `max` pt.
fn wrap(s: Style, text: &str, max: f32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = String::new();
    for word in text.split_whitespace() {
        let candidate = if cur.is_empty() { word.to_string() } else { format!("{cur} {word}") };
        if !cur.is_empty() && width(&shape(s.face, &candidate, s.tabular, s.tracking), s.size) > max {
            lines.push(std::mem::replace(&mut cur, word.to_string()));
        } else {
            cur = candidate;
        }
    }
    if !cur.is_empty() || lines.is_empty() {
        lines.push(cur);
    }
    lines
}

/// Lays the invoice out into pages of display items.
pub(crate) fn layout(inv: &Invoice) -> Vec<Vec<Draw>> {
    let mut w = Writer { pages: Vec::new(), items: Vec::new(), y: MARGIN };
    header(&mut w, inv);
    meta(&mut w, inv);
    table_head(&mut w);
    for line in inv.item_lines() {
        row(&mut w, inv, line);
    }
    totals(&mut w, inv);
    w.new_page();

    // Page footers, now that the page count is known.
    let n = w.pages.len();
    let mut pages = std::mem::take(&mut w.pages);
    for (i, items) in pages.iter_mut().enumerate() {
        let mut fw = Writer { pages: Vec::new(), items: Vec::new(), y: 0.0 };
        let base = PAGE_H - MARGIN + 6.0;
        let foot = style(Face::Regular, FOOTER, MUTED);
        fw.text(foot, Align::Left, MARGIN, base, &format!("{} · Invoice {}", inv.seller.name, inv.doc_number));
        fw.text(foot, Align::Right, PAGE_W - MARGIN, base, &format!("Page {} of {n}", i + 1));
        items.extend(fw.items);
    }
    pages
}

/// Seller block on the left, "Invoice" and the total on the right (design: the paper's first row).
fn header(w: &mut Writer, inv: &Invoice) {
    let top = w.y;
    let s = &inv.seller;
    w.items.push(Draw::Rect { x: MARGIN, y: top, w: 24.0, h: 24.0, color: EMBER });
    let initial: String = s.name.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
    w.text(style(Face::Semibold, 10.0, WHITE), Align::Center, MARGIN + 12.0, top + 15.6, &initial);
    w.text(style(Face::Semibold, BODY, CONTENT), Align::Left, MARGIN + 31.0, top + 10.0, &s.name);
    w.text(style(Face::Regular, SMALL, MUTED), Align::Left, MARGIN + 31.0, top + 21.5, &s.tagline);
    w.text(style(Face::Regular, SMALL, MUTED), Align::Left, MARGIN, top + 42.0, &s.address);
    w.text(style(Face::Regular, SMALL, MUTED), Align::Left, MARGIN, top + 56.0, &s.contact);

    let right = PAGE_W - MARGIN;
    w.eyebrow(Align::Right, right, top + 8.0, &format!("Invoice {}", inv.doc_number));
    w.text(
        num(Face::Semibold, BIG, CONTENT),
        Align::Right,
        right,
        top + 31.0,
        &inv.total_before_retainer().to_string(),
    );
    w.text(style(Face::Regular, SMALL, MUTED), Align::Right, right, top + 44.0, "Total");
    w.y = top + 62.0 + 24.0;
}

/// Bill to, dates, terms and tax in four columns, then a hairline.
fn meta(w: &mut Writer, inv: &Invoice) {
    let gap = 15.0;
    let col = (CONTENT_W - 3.0 * gap) / 4.0;
    let x = |i: usize| MARGIN + i as f32 * (col + gap);
    let lh = 14.0;
    let top = w.y + META;
    let label = style(Face::Regular, META, MUTED);
    let value = num(Face::Regular, META, CONTENT);

    w.text(label, Align::Left, x(0), top, "Bill to");
    w.text(style(Face::Medium, META, CONTENT), Align::Left, x(0), top + lh, &inv.customer);
    let mut bottom = top + lh;
    for (i, l) in inv.bill_to.iter().enumerate() {
        bottom = top + lh * (i as f32 + 2.0);
        w.text(label, Align::Left, x(0), bottom, l);
    }

    let mut pair = |col: usize, y: f32, l: &str, v: &str| {
        w.text(label, Align::Left, x(col), y, l);
        w.text(value, Align::Left, x(col), y + lh, v);
    };
    let second = top + 2.0 * lh + 9.0;
    pair(1, top, "Invoice date", &display_date(&inv.date));
    if !inv.terms.is_empty() {
        pair(1, second, "Terms", &inv.terms);
    }
    pair(2, top, "Due date", &display_date(&inv.due_date));
    pair(3, top, "Tax", &inv.tax_rate_label());
    bottom = bottom.max(second + lh);

    w.y = bottom + 4.0 + 18.0;
    w.hline(MARGIN, PAGE_W - MARGIN, w.y);
    w.y += 12.0;
}

fn table_head(w: &mut Writer) {
    let s = style(Face::Medium, TH, MUTED);
    let base = w.y + ROW_PAD + TH;
    let x_item = MARGIN + COL_DESC_W;
    let x_qty = x_item + COL_ITEM_W + COL_QTY_W;
    let x_rate = x_qty + COL_RATE_W;
    w.text(s, Align::Left, MARGIN, base, "Description");
    w.text(s, Align::Left, x_item, base, "Item");
    w.text(s, Align::Right, x_qty, base, "Qty");
    w.text(s, Align::Right, x_rate, base, "Rate");
    w.text(s, Align::Center, x_rate + COL_TAX_W / 2.0, base, "Tax");
    w.text(s, Align::Right, x_rate + COL_TAX_W + COL_AMOUNT_W, base, "Amount");
    w.y = base + ROW_PAD;
}

fn row(w: &mut Writer, inv: &Invoice, line: &Line) {
    let desc_style = style(Face::Medium, BODY, CONTENT);
    let desc = wrap(desc_style, &line.description, COL_DESC_W - 8.0);
    let height = 2.0 * ROW_PAD + ROW_LEADING * desc.len() as f32;
    if w.y + height > BOTTOM {
        w.new_page();
        w.eyebrow(Align::Left, MARGIN, w.y + EYEBROW, &format!("Invoice {} (continued)", inv.doc_number));
        w.y += EYEBROW + 10.0;
        table_head(w);
    }
    w.hline(MARGIN, PAGE_W - MARGIN, w.y);
    let base = w.y + ROW_PAD + BODY;
    for (i, d) in desc.iter().enumerate() {
        w.text(desc_style, Align::Left, MARGIN, base + ROW_LEADING * i as f32, d);
    }
    let x_item = MARGIN + COL_DESC_W;
    let x_qty = x_item + COL_ITEM_W + COL_QTY_W;
    let x_rate = x_qty + COL_RATE_W;
    w.text(style(Face::Mono, SMALL * 0.94, MUTED), Align::Left, x_item, base, &line.item);
    w.text(num(Face::Regular, BODY, CONTENT), Align::Right, x_qty, base, &line.qty_text());
    w.text(num(Face::Regular, BODY, CONTENT), Align::Right, x_rate, base, &line.rate.to_string());
    if line.taxable {
        w.text(style(Face::Regular, SMALL, MUTED), Align::Center, x_rate + COL_TAX_W / 2.0, base, "Yes");
    }
    let x_amount = x_rate + COL_TAX_W + COL_AMOUNT_W;
    w.text(num(Face::Medium, BODY, CONTENT), Align::Right, x_amount, base, &line.amount.to_string());
    w.y += height;
}

/// The totals block and the memo, kept together on one page.
fn totals(w: &mut Writer, inv: &Invoice) {
    let step = BODY + 6.0;
    let memo_style = style(Face::Regular, SMALL, MUTED);
    let memo = wrap(memo_style, &inv.memo, CONTENT_W);
    let mut rows: Vec<(String, String)> = vec![
        ("Subtotal".into(), inv.subtotal_before_retainer().to_string()),
        (format!("Sales tax ({} on taxable)", inv.tax_rate_label()), inv.totals.total_tax.to_string()),
    ];
    let retainer = inv.retainer();
    let paid = inv.paid();
    if retainer.0 != 0 {
        rows.push(("Retainer applied".into(), format!("−{}", -retainer)));
    }
    if paid.0 != 0 {
        rows.push(("Payments".into(), format!("−{paid}")));
    }
    let memo_h = if inv.memo.is_empty() { 0.0 } else { 24.0 + 15.0 + memo.len() as f32 * 14.0 };
    let height = 18.0 + step * (rows.len() as f32 + 2.0) + 12.0 + 6.0 + memo_h;
    if w.y + height > BOTTOM {
        w.new_page();
        w.eyebrow(Align::Left, MARGIN, w.y + EYEBROW, &format!("Invoice {} (continued)", inv.doc_number));
        w.y += EYEBROW + 10.0;
    }

    let right = PAGE_W - MARGIN;
    let left = right - 240.0;
    let mut y = w.y + 18.0;
    let put = |w: &mut Writer, y: &mut f32, label: &str, value: &str, strong: bool, size: f32, value_color: Rgb| {
        let face = if strong { Face::Semibold } else { Face::Regular };
        w.text(style(face, size, if strong { CONTENT } else { MUTED }), Align::Left, left, *y + size, label);
        w.text(num(face, size, value_color), Align::Right, right, *y + size, value);
        *y += size + 6.0;
    };
    for (i, (label, value)) in rows.iter().enumerate() {
        put(w, &mut y, label, value, false, BODY, CONTENT);
        if i == 1 {
            // Total, after subtotal and tax.
            w.hline(left, right, y - 1.0);
            y += 5.0;
            put(w, &mut y, "Total", &inv.total_before_retainer().to_string(), true, BODY, CONTENT);
        }
    }
    w.hline(left, right, y - 1.0);
    y += 5.0;
    put(w, &mut y, "Balance due", &inv.balance().to_string(), true, BALANCE, PRIMARY);

    if !inv.memo.is_empty() {
        y += 24.0;
        w.hline(MARGIN, right, y);
        y += 15.0;
        for l in &memo {
            w.text(memo_style, Align::Left, MARGIN, y + SMALL, l);
            y += 14.0;
        }
    }
    w.y = y;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes_wraps_and_aligns_figures() {
        let w = |t: &str, tab: bool| width(&shape(Face::Regular, t, tab, 0.0), 10.0);
        assert!(w("Discovery workshop", false) > 50.0);
        // Geist's default figures are proportional; tnum makes every amount of one shape the same width.
        assert_ne!(w("1", false), w("0", false));
        assert!((w("$1,111.11", true) - w("$8,888.88", true)).abs() < 0.001);
        let s = style(Face::Regular, 10.0, CONTENT);
        let lines = wrap(s, "one two three four five six seven eight nine ten", 60.0);
        assert!(lines.len() > 1 && lines.iter().all(|l| width(&shape(Face::Regular, l, false, 0.0), 10.0) <= 60.0));
    }

    fn texts(page: &[Draw]) -> Vec<&str> {
        page.iter().filter_map(|d| if let Draw::Text { text, .. } = d { Some(text.as_str()) } else { None }).collect()
    }

    fn invoice(doc: &str) -> Invoice {
        Invoice::load(&crate::db::open_demo(None).unwrap(), doc).unwrap()
    }

    #[test]
    fn inv_1048_prints_the_money_crate_totals_on_one_page() {
        let pages = layout(&invoice("INV-1048"));
        assert_eq!(pages.len(), 1);
        let t = texts(&pages[0]);
        for s in ["$3,550.00", "$7.15", "$3,557.15", "−$500.00", "−$1,000.00", "$2,057.15", "Page 1 of 1"] {
            assert!(t.contains(&s), "{s} in {t:?}");
        }
    }

    #[test]
    fn long_invoice_breaks_between_rows() {
        let inv = invoice(crate::invoice::LONG_FIXTURE);
        let pages = layout(&inv);
        assert_eq!(pages.len(), 4);
        for (i, page) in pages.iter().enumerate() {
            let t = texts(page);
            assert!(t.contains(&format!("Page {} of 4", i + 1).as_str()));
            if i > 0 {
                assert!(t.contains(&"INVOICE FIXTURE-60 (CONTINUED)"), "page {}", i + 1);
            }
            // Nothing but the footer below the content area.
            for d in page {
                if let Draw::Text { baseline, size, text, .. } = d {
                    assert!(*baseline <= BOTTOM + ROW_LEADING || *size == FOOTER, "{text} at {baseline}");
                }
            }
        }
        // Every line is printed once, and the balance is on the last page.
        let all: Vec<&str> = pages.iter().flat_map(|p| texts(p)).collect();
        assert_eq!(all.iter().filter(|t| **t == "Description").count(), 3, "header on pages 1-3, not the totals page");
        assert!(texts(&pages[3]).contains(&inv.balance().to_string().as_str()));
        assert_eq!(all.iter().filter(|t| t.starts_with("Discovery workshop")).count(), 15);
    }

    #[test]
    fn glyph_ranges_cover_the_text() {
        let g = shape(Face::Medium, "Office fit-out", false, 0.0);
        assert_eq!(g.first().unwrap().range.start, 0);
        assert_eq!(g.last().unwrap().range.end, "Office fit-out".len());
    }
}
