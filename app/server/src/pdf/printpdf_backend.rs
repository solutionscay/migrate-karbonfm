//! Writes the display list with printpdf 0.12 (`text_layout` feature, which is what subsets the fonts).
//! Shaped glyphs go in as glyph ids; the TJ offsets carry the difference between the shaped advance (kerning,
//! letter spacing) and the font's own advance, and `cid` carries the text for copy and search (ToUnicode).

use super::{Draw, Rgb, FACES, FONT_FILES, PAGE_H};
use printpdf::{
    Codepoint, Color, Line, LinePoint, Mm, Op, PaintMode, ParsedFont, PdfDocument, PdfFontHandle, PdfPage,
    PdfSaveOptions, Point, Pt, Rect, TextItem,
};

fn color([r, g, b]: Rgb) -> Color {
    Color::Rgb(printpdf::Rgb { r, g, b, icc_profile: None })
}

pub(super) fn write(title: &str, pages: &[Vec<Draw>]) -> Vec<u8> {
    let mut doc = PdfDocument::new(title);
    let ids: Vec<_> = FONT_FILES
        .iter()
        .map(|b| doc.add_font(&ParsedFont::from_bytes(b, 0, &mut Vec::new()).expect("vendored Geist TTF parses")))
        .collect();
    let pages = pages
        .iter()
        .map(|items| {
            let mut ops = Vec::new();
            for item in items {
                match item {
                    Draw::Text { face, size, x, baseline, text, glyphs, color: c } => {
                        let rb = &FACES[*face as usize];
                        let upem = rb.units_per_em() as f32;
                        let codepoints = glyphs
                            .iter()
                            .map(|g| {
                                let own = rb.glyph_hor_advance(rustybuzz::ttf_parser::GlyphId(g.id)).unwrap_or(0);
                                let offset = (own as f32 / upem - g.advance) * 1000.0;
                                Codepoint::with_cid(g.id, offset, text[g.range.clone()].to_string())
                            })
                            .collect();
                        ops.extend([
                            Op::StartTextSection,
                            Op::SetFillColor { col: color(*c) },
                            Op::SetFont { font: PdfFontHandle::External(ids[*face as usize].clone()), size: Pt(*size) },
                            Op::SetTextCursor { pos: Point { x: Pt(*x), y: Pt(PAGE_H - baseline) } },
                            Op::ShowText { items: vec![TextItem::GlyphIds(codepoints)] },
                            Op::EndTextSection,
                        ]);
                    }
                    Draw::Line { x1, x2, y, color: c } => {
                        let p = |x: f32| LinePoint { p: Point { x: Pt(x), y: Pt(PAGE_H - y) }, bezier: false };
                        ops.extend([
                            Op::SetOutlineColor { col: color(*c) },
                            Op::SetOutlineThickness { pt: Pt(0.75) },
                            Op::DrawLine { line: Line { points: vec![p(*x1), p(*x2)], is_closed: false } },
                        ]);
                    }
                    Draw::Rect { x, y, w, h, color: c } => ops.extend([
                        Op::SetFillColor { col: color(*c) },
                        Op::DrawRectangle {
                            rectangle: Rect {
                                x: Pt(*x),
                                y: Pt(PAGE_H - y - h),
                                width: Pt(*w),
                                height: Pt(*h),
                                mode: Some(PaintMode::Fill),
                                winding_order: None,
                            },
                        },
                    ]),
                }
            }
            PdfPage::new(Mm(215.9), Mm(279.4), ops)
        })
        .collect();
    doc.with_pages(pages).save(&PdfSaveOptions::default(), &mut Vec::new())
}
