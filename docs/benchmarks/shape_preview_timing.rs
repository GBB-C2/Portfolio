#![allow(dead_code)]
#[cfg(baseline)]
#[path = "document_before_stamp_optimization.rs"]
mod document;
#[cfg(not(baseline))]
#[path = "../../src/document.rs"]
mod document;
use document::{Brush, BrushKind, Color, Document, ShapeKind};
use std::hint::black_box;
use std::time::Instant;

fn main() {
    println!("case,width,height,size,first_ms,update_median_ms,update_max_ms,composite_median_ms,repeat_us,commit_ms");
    for (label, w, h, size, mirror, spacing, kind) in [
        ("ink",1280,800,96.0,false,0.12,BrushKind::Ink),
        ("ink",1280,800,512.0,false,0.12,BrushKind::Ink),
        ("ink",1920,1080,96.0,false,0.12,BrushKind::Ink),
        ("ink",1920,1080,512.0,false,0.12,BrushKind::Ink),
        ("dense_mirror_ink",1920,1080,512.0,true,0.02,BrushKind::Ink),
        ("dense_mirror_soft",1920,1080,512.0,true,0.02,BrushKind::Soft),
    ] {
        if std::env::args().nth(1).as_deref() == Some("hd") && (w != 1920 || label != "ink") {
            continue;
        }
        let mut doc=Document::with_background(w,h,Color::WHITE).unwrap();
        let mut composite=doc.composite();
        doc.take_paint_damage();
        let brush=Brush {size, mirror, spacing, kind, opacity:0.65, color:Color::rgb(230,40,30), ..Brush::default()};
        let (x0,y0)=(w as f32*0.15,h as f32*0.15);
        let (mut x1,mut y1)=(w as f32*0.85,h as f32*0.85);
        let start=Instant::now();
        assert!(doc.preview_shape(ShapeKind::Ellipse,x0,y0,x1,y1,brush).unwrap());
        let first_ms=start.elapsed().as_secs_f64()*1000.0;
        if let Some(damage)=doc.take_paint_damage() {doc.composite_region_into(&mut composite,damage);}
        let mut times=Vec::new();
        let mut compose=Vec::new();
        for n in 0..9 {
            x1=w as f32*0.85+(n+1) as f32*0.4;
            y1=h as f32*0.85+(n+1) as f32*0.3;
            let start=Instant::now();
            assert!(black_box(&mut doc).preview_shape(ShapeKind::Ellipse,x0,y0,x1,y1,brush).unwrap());
            times.push(start.elapsed().as_secs_f64()*1000.0);
            let start=Instant::now();
            if let Some(damage)=doc.take_paint_damage() {doc.composite_region_into(&mut composite,damage);}
            black_box(&composite);
            compose.push(start.elapsed().as_secs_f64()*1000.0);
        }
        let start=Instant::now();
        for _ in 0..10000 {
            assert!(!black_box(&mut doc).preview_shape(ShapeKind::Ellipse,x0,y0,x1,y1,brush).unwrap());
        }
        let repeat_us=start.elapsed().as_secs_f64()*1_000_000.0/10000.0;
        let start=Instant::now();
        assert!(doc.commit_shape_preview());
        let commit_ms=start.elapsed().as_secs_f64()*1000.0;
        times.sort_by(f64::total_cmp);
        compose.sort_by(f64::total_cmp);
        println!("{label},{w},{h},{size},{first_ms:.3},{:.3},{:.3},{:.3},{repeat_us:.4},{commit_ms:.3}",times[4],times[8],compose[4]);
    }
}

