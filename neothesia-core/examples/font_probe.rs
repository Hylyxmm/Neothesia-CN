// Diagnostic probe: reproduce the app's font selection pipeline without a GUI.
// Prints which font family the embedded Noto Sans CJK TTC registers as, and which
// family cosmic-text actually resolves for mixed CJK/Latin text.
use glyphon::{Attrs, Family, Metrics, Shaping};

fn probe(fs: &mut glyphon::FontSystem, label: &str, family: Family) {
    let mut buf = glyphon::Buffer::new(fs, Metrics::new(20.0, 20.0));
    buf.set_text("输入MIDI", &Attrs::new().family(family), Shaping::Advanced, None);
    buf.shape_until_scroll(fs, false);

    for run in buf.layout_runs() {
        let text: &str = run.text;
        for g in run.glyphs.iter() {
            let face = fs.db().face(g.font_id);
            let name = face
                .and_then(|f| f.families.first().map(|(n, _)| n.clone()))
                .unwrap_or_else(|| "<unknown>".to_string());
            let ch = text.get(g.start..g.end).unwrap_or("?");
            println!("{label}: char={ch:?} glyph_id={} family={name:?}", g.glyph_id);
        }
    }
}

fn probe_basic(fs: &mut glyphon::FontSystem, label: &str, family: Family) {
    let mut buf = glyphon::Buffer::new(fs, Metrics::new(20.0, 20.0));
    buf.set_text("输入MIDI", &Attrs::new().family(family), Shaping::Basic, None);
    buf.shape_until_scroll(fs, false);

    for run in buf.layout_runs() {
        let text: &str = run.text;
        for g in run.glyphs.iter() {
            let face = fs.db().face(g.font_id);
            let name = face
                .and_then(|f| f.families.first().map(|(n, _)| n.clone()))
                .unwrap_or_else(|| "<unknown>".to_string());
            let ch = text.get(g.start..g.end).unwrap_or("?");
            println!("{label}: char={ch:?} glyph_id={} family={name:?}", g.glyph_id);
        }
    }
}

fn main() {
    let fs_cell = neothesia_core::font_system::font_system();
    let mut fs = fs_cell.borrow_mut();

    let db = fs.db();
    println!("total faces in DB: {}", db.len());

    let mut noto: Vec<String> = Vec::new();
    let mut yahei: Vec<String> = Vec::new();
    for face in db.faces() {
        for (name, _) in &face.families {
            let n = name.to_lowercase();
            if n.contains("noto sans cjk") && !noto.iter().any(|f| f == name) {
                noto.push(name.clone());
            }
            if n.contains("yahei") && !yahei.iter().any(|f| f == name) {
                yahei.push(name.clone());
            }
        }
    }
    println!("Noto CJK families registered: {noto:?}");
    println!("YaHei families registered: {yahei:?}");

    probe(&mut fs, "Family::Name(\"Noto Sans CJK SC\")", Family::Name("Noto Sans CJK SC"));
    probe(&mut fs, "Family::SansSerif", Family::SansSerif);
    probe(&mut fs, "Family::Name(\"Roboto\")", Family::Name("Roboto"));
    probe_basic(&mut fs, "BASIC Noto SC", Family::Name("Noto Sans CJK SC"));
    probe_basic(&mut fs, "BASIC Roboto", Family::Name("Roboto"));
    probe_basic(&mut fs, "BASIC SansSerif", Family::SansSerif);
    probe_basic_bold(&mut fs, "BASIC BOLD Noto SC", Family::Name("Noto Sans CJK SC"));
    probe_basic_bold(&mut fs, "BASIC BOLD Roboto", Family::Name("Roboto"));
}

fn probe_basic_bold(fs: &mut glyphon::FontSystem, label: &str, family: Family) {
    let mut buf = glyphon::Buffer::new(fs, Metrics::new(20.0, 20.0));
    buf.set_text(
        "输入MIDI",
        &Attrs::new().family(family).weight(glyphon::Weight::BOLD),
        Shaping::Basic,
        None,
    );
    buf.shape_until_scroll(fs, false);

    for run in buf.layout_runs() {
        let text: &str = run.text;
        for g in run.glyphs.iter() {
            let face = fs.db().face(g.font_id);
            let name = face
                .and_then(|f| f.families.first().map(|(n, _)| n.clone()))
                .unwrap_or_else(|| "<unknown>".to_string());
            let ch = text.get(g.start..g.end).unwrap_or("?");
            println!("{label}: char={ch:?} glyph_id={} family={name:?}", g.glyph_id);
        }
    }
}
