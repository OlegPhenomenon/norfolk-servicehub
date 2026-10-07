//! Simple generated PDFs (decisions, letters, receipts, certificates) shared by all modules.
//! A4, built-in Helvetica, word-wrapped, multi-page, with the demonstration header and footer.

use printpdf::{BuiltinFont, Color, IndirectFontRef, Line, Mm, PdfDocument, PdfLayerReference, Point, Rgb};

const PAGE_W: f32 = 210.0;
const PAGE_H: f32 = 297.0;
const MARGIN_X: f32 = 20.0;
const TOP_Y: f32 = 272.0;
const BOTTOM_Y: f32 = 24.0;
const PT_TO_MM: f32 = 0.3528;

/// Header printed on every page.
pub const HEADER: &str = "Norfolk ServiceHub — demonstration";
/// Footer printed on every page.
pub const FOOTER: &str = "Fictional demonstration document — not issued by Norfolk Island Regional Council";

/// Helvetica advance widths (1/1000 em) for ASCII 32..=126.
const HELVETICA_WIDTHS: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, // ' '..'/'
    556, 556, 556, 556, 556, 556, 556, 556, 556, 556, // 0-9
    278, 278, 584, 584, 584, 556, 1015, // ':'..'@'
    667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944,
    667, 667, 611, // A-Z
    278, 278, 278, 469, 556, 333, // '['..'`'
    556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556, 556, 556, 333, 500, 278, 556, 500, 722,
    500, 500, 500, // a-z
    334, 260, 334, 584, // '{'..'~'
];

fn char_width(c: char, bold: bool) -> f32 {
    let w = match c as u32 {
        32..=126 => HELVETICA_WIDTHS[(c as u32 - 32) as usize] as f32,
        0x2014 => 1000.0,
        _ => 556.0,
    };
    if bold { w * 1.06 } else { w }
}

/// Width of `s` in mm at `size` pt.
fn text_width(s: &str, size: f32, bold: bool) -> f32 {
    s.chars().map(|c| char_width(c, bold)).sum::<f32>() / 1000.0 * size * PT_TO_MM
}

/// Keeps characters the WinAnsi-encoded built-in fonts can show; replaces the rest.
fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '\t' => ' ',
            '\u{20}'..='\u{7e}' | '\u{a0}'..='\u{ff}' => c,
            '—' | '–' | '‘' | '’' | '“' | '”' | '•' | '…' | '€' => c,
            '\u{2010}' | '\u{2011}' | '\u{2212}' => '-',
            _ => '?',
        })
        .collect()
}

/// Greedy word wrap to `max_mm`; very long words are split.
fn wrap(text: &str, size: f32, bold: bool, max_mm: f32) -> Vec<String> {
    let mut lines = Vec::new();
    for para in text.split('\n') {
        let mut line = String::new();
        for word in para.split_whitespace() {
            let candidate = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
            if text_width(&candidate, size, bold) <= max_mm {
                line = candidate;
                continue;
            }
            if !line.is_empty() {
                lines.push(std::mem::take(&mut line));
            }
            // Split a word longer than the line.
            let mut chunk = String::new();
            for c in word.chars() {
                chunk.push(c);
                if text_width(&chunk, size, bold) > max_mm {
                    chunk.pop();
                    lines.push(std::mem::take(&mut chunk));
                    chunk.push(c);
                }
            }
            line = chunk;
        }
        lines.push(line);
    }
    lines
}

struct Writer {
    doc: printpdf::PdfDocumentReference,
    layer: PdfLayerReference,
    regular: IndirectFontRef,
    bold: IndirectFontRef,
    y: f32,
    pages: usize,
}

impl Writer {
    fn decorate(&self) {
        let grey = Color::Rgb(Rgb::new(0.35, 0.35, 0.35, None));
        self.layer.set_fill_color(grey.clone());
        self.layer.use_text(sanitize(HEADER), 9.0, Mm(MARGIN_X), Mm(PAGE_H - 14.0), &self.regular);
        self.layer.set_outline_color(grey);
        self.layer.set_outline_thickness(0.5);
        self.layer.add_line(Line {
            points: vec![
                (Point::new(Mm(MARGIN_X), Mm(PAGE_H - 17.0)), false),
                (Point::new(Mm(PAGE_W - MARGIN_X), Mm(PAGE_H - 17.0)), false),
            ],
            is_closed: false,
        });
        self.layer.use_text(sanitize(FOOTER), 8.0, Mm(MARGIN_X), Mm(12.0), &self.regular);
        let page = format!("Page {}", self.pages);
        let w = text_width(&page, 8.0, false);
        self.layer.use_text(page, 8.0, Mm(PAGE_W - MARGIN_X - w), Mm(12.0), &self.regular);
        self.layer.set_fill_color(Color::Rgb(Rgb::new(0.0, 0.0, 0.0, None)));
    }

    fn new_page(&mut self) {
        let (page, layer) = self.doc.add_page(Mm(PAGE_W), Mm(PAGE_H), "Layer 1");
        self.layer = self.doc.get_page(page).get_layer(layer);
        self.pages += 1;
        self.y = TOP_Y;
        self.decorate();
    }

    fn ensure(&mut self, needed_mm: f32) {
        if self.y - needed_mm < BOTTOM_Y {
            self.new_page();
        }
    }

    fn paragraph(&mut self, text: &str, size: f32, bold: bool, indent: f32) {
        let line_h = size * 1.4 * PT_TO_MM;
        for line in wrap(&sanitize(text), size, bold, PAGE_W - 2.0 * MARGIN_X - indent) {
            self.ensure(line_h);
            self.y -= line_h;
            let font = if bold { &self.bold } else { &self.regular };
            self.layer.use_text(line, size, Mm(MARGIN_X + indent), Mm(self.y), font);
        }
    }

    fn gap(&mut self, mm: f32) {
        self.y -= mm;
    }
}

/// Builds a PDF: `title`, then `meta` as "Label: value" lines, then each `(heading, body)` section.
/// Body text is word-wrapped; `\n` starts a new line. Pages break automatically.
pub fn simple_document(title: &str, meta: &[(&str, String)], sections: &[(&str, String)]) -> Vec<u8> {
    let (doc, page, layer) = PdfDocument::new(sanitize(title), Mm(PAGE_W), Mm(PAGE_H), "Layer 1");
    let regular = doc.add_builtin_font(BuiltinFont::Helvetica).expect("builtin font");
    let bold = doc.add_builtin_font(BuiltinFont::HelveticaBold).expect("builtin font");
    let layer = doc.get_page(page).get_layer(layer);
    let mut w = Writer { doc, layer, regular, bold, y: TOP_Y, pages: 1 };
    w.decorate();

    w.paragraph(title, 18.0, true, 0.0);
    w.gap(4.0);
    for (label, value) in meta {
        w.paragraph(&format!("{label}: {value}"), 10.0, false, 0.0);
    }
    if !meta.is_empty() {
        w.gap(4.0);
    }
    for (heading, body) in sections {
        w.ensure(14.0);
        w.gap(2.0);
        w.paragraph(heading, 12.0, true, 0.0);
        w.gap(1.0);
        w.paragraph(body, 10.5, false, 0.0);
        w.gap(3.0);
    }
    w.doc.save_to_bytes().expect("pdf serialisation")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_multi_page_pdf() {
        let long = "The applicant must keep the hall clean and return the keys. ".repeat(200);
        let bytes = simple_document(
            "Venue hire confirmation — Rawson Hall",
            &[("Reference", "NSH-2026-000001".into()), ("Applicant", "Alexey Turner".into())],
            &[("Conditions", long), ("Notes", "Short.".into())],
        );
        assert!(bytes.starts_with(b"%PDF"));
        assert!(bytes.len() > 2000);
        assert_eq!(infer::get(&bytes).map(|t| t.mime_type()), Some("application/pdf"));
    }

    #[test]
    fn wraps_and_sanitizes() {
        let lines = wrap("one two three four five six seven", 10.0, false, 20.0);
        assert!(lines.len() > 1);
        assert!(lines.iter().all(|l| text_width(l, 10.0, false) <= 20.0));
        assert_eq!(sanitize("a\u{4e2d}b—c"), "a?b—c");
    }
}
