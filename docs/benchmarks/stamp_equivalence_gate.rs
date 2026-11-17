#![allow(dead_code)]
#[path="document_before_stamp_optimization.rs"]
mod before;
#[path="../../src/document.rs"]
mod current;
use std::io::{BufWriter, Write};

macro_rules! run_case {
    ($module:ident, $kind:expr, $erase:expr, $mirror:expr, $opacity:expr, $size:expr) => {{
        use $module::{Brush, BrushKind, Color, Document, ShapeKind};
        fn frame(doc: &Document) -> Vec<u8> {
            let mut bytes = Vec::with_capacity(doc.layers[0].pixels.len()*4+32);
            bytes.extend_from_slice(&(doc.undo_count() as u64).to_le_bytes());
            bytes.extend_from_slice(&(doc.redo_count() as u64).to_le_bytes());
            bytes.push(doc.is_dirty() as u8);
            for layer in &doc.layers {
                bytes.push(layer.opacity);
                for p in &layer.pixels { bytes.extend_from_slice(&[p.r,p.g,p.b,p.a]); }
            }
            bytes
        }
        let pixels=(0..151*109).map(|i| Color::rgba(
            (i*13%256) as u8,(i*31%256) as u8,(i*7%256) as u8,
            match i%5 {0=>0,1=>255,_=>(i*17%256) as u8}
        )).collect();
        let mut doc=Document::from_pixels(151,109,pixels).unwrap();
        let brush=Brush {
            kind: BrushKind::from_index($kind), erase:$erase, mirror:$mirror,
            opacity:$opacity, size:$size, flow:0.61, hardness:0.58, spacing:0.07,
            angle:31.0, scatter:0.23,
            color:Color::rgba(210,35,90,if $erase && $opacity == 1.0 {0} else if $mirror {171} else {255}),
        };
        let mut frames=Vec::new();
        if $size >= 512.0 {
            doc.begin_stroke(-1e30,54.0,brush).unwrap();
            doc.continue_stroke(1e30,54.0);
            doc.continue_stroke(75.25,54.0);
            doc.continue_stroke(77.9,55.1);
            doc.end_stroke();
            frames.push(frame(&doc));
        }
        doc.begin_stroke(7.25,7.75,brush).unwrap();
        doc.continue_stroke(138.6,99.2);
        doc.continue_stroke(-25.4,80.2);
        doc.continue_stroke(177.2,12.4);
        doc.end_stroke();
        frames.push(frame(&doc));
        doc.preview_shape(ShapeKind::Line,-13.2,7.6,147.7,102.1,brush).unwrap();
        frames.push(frame(&doc));
        doc.preview_shape(ShapeKind::Rectangle,-11.4,8.2,147.5,110.7,brush).unwrap();
        frames.push(frame(&doc));
        doc.preview_shape(ShapeKind::Ellipse,9.4,-12.3,168.6,99.5,brush).unwrap();
        frames.push(frame(&doc));
        doc.commit_shape_preview();
        frames.push(frame(&doc));
        doc.undo();
        frames.push(frame(&doc));
        doc.redo();
        frames.push(frame(&doc));
        doc.preview_shape(ShapeKind::Ellipse,17.4,21.2,104.5,79.7,brush).unwrap();
        doc.cancel_shape_preview();
        frames.push(frame(&doc));
        frames
    }}
}
fn checksum(bytes:&[u8])->u64 {
    bytes.iter().fold(0xcbf29ce484222325u64,|h,b|(h ^ *b as u64).wrapping_mul(0x100000001b3))
}
fn main() {
    let args:Vec<_>=std::env::args().collect();
    let compare=args.get(2).is_some_and(|s|s=="compare");
    let mut out=BufWriter::new(std::fs::File::create(&args[1]).unwrap());
    writeln!(out,"kind,erase,mirror,opacity,size,phase,fnv1a64").unwrap();
    let mut cases=0usize;
    let mut frames=0usize;
    for kind in 0u8..10 {
        for erase in [false,true] {
            for mirror in [false,true] {
                for opacity in [0.0,0.37,1.0] {
                    for size in [3.0,37.0,96.0,512.0] {
                        let expected=run_case!(before,kind,erase,mirror,opacity,size);
                        if compare {
                            let actual=run_case!(current,kind,erase,mirror,opacity,size);
                            for (phase,(a,b)) in expected.iter().zip(&actual).enumerate() {
                                assert_eq!(a,b,"pixel/state mismatch kind={kind} erase={erase} mirror={mirror} opacity={opacity} size={size} phase={phase}");
                            }
                        }
                        for (phase,data) in expected.iter().enumerate() {
                            writeln!(out,"{kind},{erase},{mirror},{opacity},{size},{phase},{:016x}",checksum(data)).unwrap();
                            frames+=1;
                        }
                        cases+=1;
                    }
                }
            }
        }
    }
    out.flush().unwrap();
    println!("{cases} configurations, {frames} frames; exact comparison={compare}; wrote {}",args[1]);
}

