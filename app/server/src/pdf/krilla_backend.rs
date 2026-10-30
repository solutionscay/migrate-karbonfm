//! Writes the display list with krilla 0.7 (the PDF writer of Typst). krilla subsets the fonts and writes
//! ToUnicode from the glyph text ranges itself; its coordinates run down from the top, like the layout's.

use super::{Draw, Rgb, FONT_FILES, PAGE_H, PAGE_W};
use krilla::color::rgb;
use krilla::geom::{PathBuilder, Point, Rect};
use krilla::metadata::Metadata;
use krilla::page::PageSettings;
use krilla::paint::{Fill, Stroke};
use krilla::text::{Font, GlyphId, KrillaGlyph};
use krilla::Document;

fn paint([r, g, b]: Rgb) -> krilla::paint::Paint {
    let c = |v: f32| (v * 255.0).round() as u8;
    rgb::Color::new(c(r), c(g), c(b)).into()
}

pub(super) fn write(title: &str, pages: &[Vec<Draw>]) -> Vec<u8> {
    let mut doc = Document::new();
    doc.set_metadata(Metadata::new().title(title.to_string()));
    let fonts: Vec<Font> =
        FONT_FILES.iter().map(|b| Font::new((*b).into(), 0).expect("vendored Geist TTF parses")).collect();
    for items in pages {
        let mut page = doc.start_page_with(PageSettings::from_wh(PAGE_W, PAGE_H).expect("valid page size"));
        let mut s = page.surface();
        for item in items {
            match item {
                Draw::Text { face, size, x, baseline, text, glyphs, color } => {
                    let glyphs: Vec<KrillaGlyph> = glyphs
                        .iter()
                        .map(|g| {
                            KrillaGlyph::new(GlyphId::new(g.id.into()), g.advance, 0.0, 0.0, 0.0, g.range.clone(), None)
                        })
                        .collect();
                    s.set_fill(Some(Fill { paint: paint(*color), ..Default::default() }));
                    s.draw_glyphs(
                        Point::from_xy(*x, *baseline),
                        &glyphs,
                        fonts[*face as usize].clone(),
                        text,
                        *size,
                        false,
                    );
                }
                Draw::Line { x1, x2, y, color } => {
                    let mut pb = PathBuilder::new();
                    pb.move_to(*x1, *y);
                    pb.line_to(*x2, *y);
                    if let Some(path) = pb.finish() {
                        s.set_fill(None);
                        s.set_stroke(Some(Stroke { paint: paint(*color), width: 0.75, ..Default::default() }));
                        s.draw_path(&path);
                        s.set_stroke(None);
                    }
                }
                Draw::Rect { x, y, w, h, color } => {
                    let mut pb = PathBuilder::new();
                    if let Some(rect) = Rect::from_xywh(*x, *y, *w, *h) {
                        pb.push_rect(rect);
                    }
                    if let Some(path) = pb.finish() {
                        s.set_fill(Some(Fill { paint: paint(*color), ..Default::default() }));
                        s.draw_path(&path);
                    }
                }
            }
        }
        s.finish();
        page.finish();
    }
    doc.finish().expect("krilla writes the invoice")
}
