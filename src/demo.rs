//! Original editable first-run poster. The embedded data contains raster pixels,
//! including the typography, so native painting and PNG export see identical art.

use crate::document::{Color, Document};

const WIDTH: u32 = 1200;
const HEIGHT: u32 = 840;
const PIXELS: usize = (WIDTH * HEIGHT) as usize;

/// Build four real document layers, leaving a blank painting layer selected.
pub fn document() -> Document {
    let mut doc = Document::with_background(WIDTH, HEIGHT, Color::from_hex(0xeee8d8))
        .expect("the embedded demo has valid dimensions");
    doc.title = "FORM 01 — этюд".into();
    doc.layers[1].name = "Формы".into();
    doc.layers[1].pixels = decode_rle(include_bytes!("../assets/demo_forms.rle"));
    let type_layer = doc
        .add_layer("Типографика")
        .expect("the embedded demo fits the layer budget");
    doc.layers[type_layer].pixels = decode_rle(include_bytes!("../assets/demo_type.rle"));
    doc.add_layer("Ваш штрих")
        .expect("the embedded demo fits the layer budget");
    doc.clear_history();
    doc.mark_saved();
    doc
}

/// Each trusted embedded record is a little-endian u32 run followed by RGBA.
fn decode_rle(bytes: &[u8]) -> Vec<Color> {
    assert_eq!(bytes.len() % 8, 0, "embedded layer record alignment");
    let mut pixels = Vec::with_capacity(PIXELS);
    for record in bytes.chunks_exact(8) {
        let count = u32::from_le_bytes([record[0], record[1], record[2], record[3]]) as usize;
        assert!(
            count > 0 && count <= PIXELS - pixels.len(),
            "embedded layer run size"
        );
        let color = Color::rgba(record[4], record[5], record[6], record[7]);
        pixels.resize(pixels.len() + count, color);
    }
    assert_eq!(pixels.len(), PIXELS, "embedded layer dimensions");
    pixels
}
