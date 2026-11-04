//! Port of `tests/StrokeSvgRaster.java`: Rasterize an exported SVG as a
//! document, outside the imported-asset profile. Args: IN.svg OUT.png

use resvg::{tiny_skia, usvg};

fn run(args: &[String]) -> Result<(), String> {
    let source = std::fs::read_to_string(&args[0]).map_err(|e| format!("{}: {}", args[0], e))?;
    let options = usvg::Options {
        fontdb: notemaker_cli::typeface::svg_fontdb().0,
        ..usvg::Options::default()
    };
    let tree = usvg::Tree::from_str(&source, &options)
        .map_err(|_| "java.lang.IllegalArgumentException: Invalid exported SVG".to_string())?;
    let mut image = tiny_skia::Pixmap::new(200, 200).ok_or("pixmap")?;
    // jsvg render(…, ViewBox(0,0,200,200)): fit the document (xMidYMid meet).
    let size = tree.size();
    let scale = (200.0 / size.width()).min(200.0 / size.height());
    let transform = tiny_skia::Transform::from_row(
        scale,
        0.0,
        0.0,
        scale,
        (200.0 - size.width() * scale) / 2.0,
        (200.0 - size.height() * scale) / 2.0,
    );
    resvg::render(&tree, transform, &mut image.as_mut());
    image.save_png(&args[1]).map_err(|e| e.to_string())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(e) = run(&args) {
        eprintln!("Exception in thread \"main\" {}", e);
        std::process::exit(1);
    }
}
