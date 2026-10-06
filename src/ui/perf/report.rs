//! One-shot HTML loop-profile report (the `--measure N` artifact).
//!
//! When the TUI is launched with `CP_MEASURE_LOOPS=N` (via `./run.sh --measure
//! N`), the main loop counts iterations and, on reaching `N`, dumps a
//! self-contained HTML page to `./tmp/loop-profile.html` with three pie
//! ("camembert") charts — mean µs, variance µs², and max µs per loop substep —
//! then the process exits. The file is deliberately under `./tmp/` (gitignored)
//! so the report is never tracked.
//!
//! The page embeds inline SVG only (no JS, no CDN) so it renders offline.
//! Non-ASCII glyphs in literals are `\u{..}`-escaped (`non_ascii_literal`).

use super::{OpSnapshot, PerfSnapshot};
use cp_base::cast::float_math;
use std::fmt::Write as _;

/// Output path for the one-shot report (realm-relative, gitignored `./tmp`).
pub(crate) const REPORT_PATH: &str = "tmp/loop-profile.html";

/// Slice colours cycled across substeps (colour-blind-friendly-ish palette).
const PALETTE: [&str; 12] = [
    "#4e79a7", "#f28e2b", "#e15759", "#76b7b2", "#59a14f", "#edc948", "#b07aa1", "#ff9da7", "#9c755f", "#bab0ac",
    "#86bcb6", "#d37295",
];

/// Write the loop-profile HTML report to [`REPORT_PATH`].
///
/// Returns the path on success; directory-creation and write errors are
/// propagated so the caller can log them.
pub(crate) fn write_report(snapshot: &PerfSnapshot) -> std::io::Result<&'static str> {
    std::fs::create_dir_all("tmp")?;
    let html = render_html(snapshot);
    std::fs::write(REPORT_PATH, html)?;
    Ok(REPORT_PATH)
}

/// Build the complete self-contained HTML document.
fn render_html(snapshot: &PerfSnapshot) -> String {
    let ops = &snapshot.ops;
    let mean_pie = pie_svg(ops, |op| op.mean_us);
    let var_pie = pie_svg(ops, |op| op.variance_us2);
    let max_pie = pie_svg(ops, |op| op.max_us);
    let legend = legend_html(ops);
    let table = table_html(ops);

    let mut out = String::with_capacity(8192);
    let _w = write!(
        out,
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
<title>Loop profile \u{2014} {loops} iterations</title>\
<style>\
body{{font:14px/1.5 -apple-system,Segoe UI,Roboto,sans-serif;margin:2rem;color:#1a1a1a;background:#fafafa}}\
h1{{font-size:1.4rem}}h2{{font-size:1rem;margin:.2rem 0}}\
.sub{{color:#666;margin-bottom:1.5rem}}\
.charts{{display:flex;flex-wrap:wrap;gap:2rem;align-items:flex-start}}\
.chart{{text-align:center}}\
.legend{{display:flex;flex-wrap:wrap;gap:.4rem 1rem;max-width:40rem;margin:1rem 0}}\
.legend span{{display:inline-flex;align-items:center;gap:.4rem;font-size:.85rem}}\
.sw{{width:.8rem;height:.8rem;border-radius:2px;display:inline-block}}\
table{{border-collapse:collapse;margin-top:1.5rem;font-size:.85rem}}\
th,td{{padding:.3rem .7rem;text-align:right;border-bottom:1px solid #ddd}}\
th:first-child,td:first-child{{text-align:left}}\
</style></head><body>\
<h1>Main-loop substep profile</h1>\
<div class=\"sub\">Rolling lifetime over <b>{loops}</b> loop iterations. \
Mean &amp; max are exact over all samples; variance is population variance (\u{b5}s\u{b2}).</div>\
<div class=\"charts\">\
<div class=\"chart\"><h2>Mean (\u{b5}s)</h2>{mean_pie}</div>\
<div class=\"chart\"><h2>Variance (\u{b5}s\u{b2})</h2>{var_pie}</div>\
<div class=\"chart\"><h2>Max (\u{b5}s)</h2>{max_pie}</div>\
</div>\
<div class=\"legend\">{legend}</div>\
{table}\
</body></html>",
        loops = snapshot.loop_count,
    );
    out
}

/// Render one inline-SVG pie chart; slice angle ∝ `value(op)`.
fn pie_svg(ops: &[OpSnapshot], value: impl Fn(&OpSnapshot) -> f64) -> String {
    let total: f64 = ops.iter().map(&value).sum();
    let mut svg = String::with_capacity(1024);
    let _open = write!(svg, "<svg width=\"220\" height=\"220\" viewBox=\"-1.05 -1.05 2.1 2.1\">");
    if total <= 0.0f64 {
        let _empty = write!(svg, "<circle r=\"1\" fill=\"#eee\"/></svg>");
        return svg;
    }
    // Walk slices accumulating the running angle; each slice is an SVG path arc.
    let mut acc = 0.0f64;
    for (op, &colour) in ops.iter().zip(PALETTE.iter().cycle()) {
        let frac = float_math::div(value(op), total);
        if frac <= 0.0f64 {
            continue;
        }
        let start = acc;
        acc = float_math::add(acc, frac);
        // A single full-circle slice can't be drawn as an arc (start==end).
        if frac >= 0.999f64 {
            let _circle = write!(svg, "<circle r=\"1\" fill=\"{colour}\"/>");
            continue;
        }
        let (x0, y0) = unit_point(start);
        let (x1, y1) = unit_point(acc);
        let large = i32::from(frac > 0.5f64);
        let _arc =
            write!(svg, "<path d=\"M0 0 L{x0:.4} {y0:.4} A1 1 0 {large} 1 {x1:.4} {y1:.4} Z\" fill=\"{colour}\"/>");
    }
    let _close = write!(svg, "</svg>");
    svg
}

/// Point on the unit circle at `frac` of a full turn, starting at 12 o'clock,
/// clockwise (SVG y-down).
fn unit_point(frac: f64) -> (f64, f64) {
    let theta = float_math::mul(frac, std::f64::consts::TAU);
    (theta.sin(), float_math::mul(theta.cos(), -1.0f64))
}

/// Colour-keyed legend mapping each substep name to its palette swatch.
fn legend_html(ops: &[OpSnapshot]) -> String {
    let mut out = String::new();
    for (op, &colour) in ops.iter().zip(PALETTE.iter().cycle()) {
        let _w = write!(out, "<span><i class=\"sw\" style=\"background:{colour}\"></i>{}</span>", esc(op.name));
    }
    out
}

/// Full numeric table: count / mean / variance / std / max per substep.
fn table_html(ops: &[OpSnapshot]) -> String {
    let mut out = String::from(
        "<table><thead><tr><th>Substep</th><th>Samples</th><th>Mean \u{b5}s</th>\
<th>Variance \u{b5}s\u{b2}</th><th>Std \u{b5}s</th><th>Max \u{b5}s</th></tr></thead><tbody>",
    );
    for op in ops {
        let _w = write!(
            out,
            "<tr><td>{}</td><td>{}</td><td>{:.1}</td><td>{:.1}</td><td>{:.1}</td><td>{:.0}</td></tr>",
            esc(op.name),
            op.count,
            op.mean_us,
            op.variance_us2,
            op.variance_us2.sqrt(),
            op.max_us,
        );
    }
    out.push_str("</tbody></table>");
    out
}

/// Minimal HTML-escape for substep names (static strings, but kept safe).
fn esc(raw: &str) -> String {
    raw.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}
