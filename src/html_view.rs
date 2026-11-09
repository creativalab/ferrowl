//! HTML mail rendering with Blitz (Stylo CSS + Taffy layout + Vello CPU).
//!
//! A dedicated worker thread owns the parsed document (Blitz documents are
//! not `Send`): it renders the message into image tiles for the reader and
//! answers hit tests ("which link is under this point?") for clicks/hover.
//!
//! - Scripts never run (Blitz has no JS engine); forms do nothing.
//! - Remote resources (images, stylesheets, fonts) are fetched over
//!   http(s) only when allowed; `cid:` images come from the message parts.
//! - Long messages are split into tiles to stay below GPU texture limits.

use anyrender::render_to_buffer;
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::DocumentConfig;
use blitz_html::HtmlDocument;
use blitz_traits::net::{Bytes, NetHandler, NetProvider, Request};
use blitz_traits::shell::{ColorScheme, Viewport};
use iced::widget::image;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Tile height in physical pixels (well below the usual 8192 texture limit).
const TILE: u32 = 2048;
/// Longest message rendered, in logical pixels.
const MAX_HEIGHT: u32 = 40_000;
const MAX_FETCHES: usize = 80;
const MAX_RESOURCE_BYTES: u64 = 8 << 20;
const FETCH_DEADLINE: Duration = Duration::from_secs(10);

#[derive(Debug, Clone)]
pub struct Rendered {
    pub uid: u32,
    /// Logical width the message was asked for (and is shown at).
    pub width: u32,
    /// Shown size / painted size: below 1 when the content is wider than
    /// `width` (fixed-width newsletters) and is shrunk to fit, like mail
    /// apps do. Tile heights are in painted units.
    pub zoom: f32,
    /// Logical height of each tile, in order.
    pub tiles: Vec<(image::Handle, f32)>,
    /// Remote content was present but not loaded.
    pub blocked_remote: bool,
    /// Some text is selected (Ctrl+C copies it).
    pub selection: bool,
}

pub struct Job {
    pub uid: u32,
    pub html: String,
    pub width: u32,
    pub scale: f32,
    pub allow_remote: bool,
    /// Inline parts by Content-ID (without angle brackets).
    pub inline: HashMap<String, Vec<u8>>,
}

enum Command {
    Render(Job, mpsc::Sender<Rendered>),
    /// Link at logical (x, y) of message `uid`.
    Hit(u32, f32, f32, mpsc::Sender<Option<String>>),
    /// Changes the text selection of message `uid` and paints it again.
    Select(u32, Select, mpsc::Sender<Option<Rendered>>),
    /// The selected text of message `uid`.
    Selected(u32, mpsc::Sender<Option<String>>),
}

/// A change to the text selection, at logical document coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Select {
    /// Mouse pressed: the selection starts (empty) here.
    Start(f32, f32),
    /// Mouse dragged: the selection now ends here.
    Extend(f32, f32),
    /// Double click: the word under the point.
    Word(f32, f32),
}

fn worker() -> &'static Mutex<mpsc::Sender<Command>> {
    static WORKER: OnceLock<Mutex<mpsc::Sender<Command>>> = OnceLock::new();
    WORKER.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Command>();
        std::thread::Builder::new()
            .name("html-render".into())
            .spawn(move || run(rx))
            .expect("spawning the HTML renderer");
        Mutex::new(tx)
    })
}

fn send(cmd: Command) {
    let _ = worker().lock().unwrap_or_else(|p| p.into_inner()).send(cmd);
}

/// Renders a message (blocking: call from a background task).
pub fn render(job: Job) -> Option<Rendered> {
    let (tx, rx) = mpsc::channel();
    send(Command::Render(job, tx));
    rx.recv().ok()
}

/// The link under a point of the last rendered message (blocking).
pub fn link_at(uid: u32, x: f32, y: f32) -> Option<String> {
    let (tx, rx) = mpsc::channel();
    send(Command::Hit(uid, x, y, tx));
    rx.recv().ok().flatten()
}

/// Applies a selection change to the last rendered message and paints
/// it again (blocking). `None` when nothing visible changed.
pub fn select(uid: u32, op: Select) -> Option<Rendered> {
    let (tx, rx) = mpsc::channel();
    send(Command::Select(uid, op, tx));
    rx.recv().ok().flatten()
}

/// The text selected in the last rendered message (blocking).
pub fn selected_text(uid: u32) -> Option<String> {
    let (tx, rx) = mpsc::channel();
    send(Command::Selected(uid, tx));
    rx.recv().ok().flatten()
}

/// A parsed message kept by the worker, so a new width only lays it out
/// and paints it again (no parsing, no downloads).
struct Loaded {
    uid: u32,
    allow_remote: bool,
    html_len: usize,
    doc: HtmlDocument,
    blocked_remote: bool,
    /// How it was last painted, to paint it again after a selection
    /// change without a new layout.
    painted: Option<Painted>,
}

#[derive(Clone, Copy)]
struct Painted {
    width: u32,
    zoom: f32,
    scale: f32,
    physical_w: u32,
    physical_h: u32,
}

fn run(rx: mpsc::Receiver<Command>) {
    let mut current: Option<Loaded> = None;
    while let Ok(cmd) = rx.recv() {
        match cmd {
            Command::Render(job, reply) => {
                // Only the most recent request matters: skip stale ones.
                let mut job = job;
                let mut reply = reply;
                while let Ok(next) = rx.try_recv() {
                    match next {
                        Command::Render(j, r) => {
                            job = j;
                            reply = r;
                        }
                        Command::Hit(_, _, _, r) => {
                            let _ = r.send(None);
                        }
                        Command::Select(_, _, r) => {
                            let _ = r.send(None);
                        }
                        Command::Selected(_, r) => {
                            let _ = r.send(None);
                        }
                    }
                }
                let same = current.as_ref().is_some_and(|c| {
                    c.uid == job.uid && c.allow_remote == job.allow_remote && c.html_len == job.html.len()
                });
                if !same {
                    current = Some(load(&job));
                }
                let loaded = current.as_mut().expect("loaded above");
                let _ = reply.send(paint(loaded, job.width, job.scale));
            }
            Command::Hit(uid, x, y, reply) => {
                let link = match &current {
                    Some(c) if c.uid == uid => link_in(&c.doc, x, y),
                    _ => None,
                };
                let _ = reply.send(link);
            }
            Command::Select(uid, op, reply) => {
                let rendered = match &mut current {
                    Some(c) if c.uid == uid => {
                        if apply(&mut c.doc, op) {
                            repaint(c)
                        } else {
                            None
                        }
                    }
                    _ => None,
                };
                let _ = reply.send(rendered);
            }
            Command::Selected(uid, reply) => {
                let text = match &current {
                    Some(c) if c.uid == uid => c.doc.get_selected_text(),
                    _ => None,
                };
                let _ = reply.send(text);
            }
        }
    }
}

/// Changes the selection; false when it stayed as it was.
fn apply(doc: &mut HtmlDocument, op: Select) -> bool {
    let before = doc.get_text_selection_ranges();
    match op {
        Select::Start(x, y) => match doc.find_text_position(x, y) {
            Some((node, offset)) => doc.set_text_selection(node, offset, node, offset),
            None => doc.clear_text_selection(),
        },
        Select::Extend(x, y) => {
            doc.extend_text_selection_to_point(x, y);
        }
        Select::Word(x, y) => match doc.find_text_position(x, y) {
            Some((node, offset)) => {
                let text = doc
                    .get_node(node)
                    .and_then(|n| n.element_data())
                    .and_then(|e| e.inline_layout_data.as_ref())
                    .map(|l| l.text.clone())
                    .unwrap_or_default();
                let (start, end) = word_at(&text, offset);
                doc.set_text_selection(node, start, node, end);
            }
            None => doc.clear_text_selection(),
        },
    }
    doc.get_text_selection_ranges() != before
}

/// Byte range of the word around `offset` (letters, digits, `_`, and the
/// `.`/`@`/`-` inside addresses and codes).
fn word_at(text: &str, offset: usize) -> (usize, usize) {
    let offset = offset.min(text.len());
    let is_word = |c: char| c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '@');
    let mut start = offset;
    for (i, c) in text[..offset].char_indices().rev() {
        if !is_word(c) {
            break;
        }
        start = i;
    }
    let mut end = offset;
    for (i, c) in text[offset..].char_indices() {
        if !is_word(c) {
            break;
        }
        end = offset + i + c.len_utf8();
    }
    // Trailing dots and dashes end sentences, not words.
    while end > start && text[..end].ends_with(['.', '-']) {
        end -= 1;
    }
    (start, end)
}

fn link_in(doc: &HtmlDocument, x: f32, y: f32) -> Option<String> {
    let hit = doc.hit(x, y)?;
    let mut node = doc.get_node(hit.node_id);
    while let Some(n) = node {
        if let Some(el) = n.element_data()
            && el.name.local == blitz_dom::local_name!("a")
            && let Some(href) = el.attr(blitz_dom::local_name!("href"))
        {
            return Some(href.trim().to_string());
        }
        node = n.parent.and_then(|p| doc.get_node(p));
    }
    None
}

/// Fetches resources on background threads; counts what is in flight.
struct Net {
    allow_remote: bool,
    inline: HashMap<String, Bytes>,
    pending: Arc<AtomicUsize>,
    started: AtomicUsize,
    blocked: Arc<AtomicUsize>,
}

impl NetProvider for Net {
    fn fetch(&self, _doc: usize, request: Request, handler: Box<dyn NetHandler>) {
        let url = request.url.to_string();
        if let Some(cid) = url.strip_prefix("cid:") {
            let cid = percent_decode(cid);
            if let Some(data) = self.inline.get(cid.trim_matches(['<', '>'])) {
                handler.bytes(url, data.clone());
            }
            return;
        }
        let remote = url.starts_with("https://") || url.starts_with("http://");
        if !remote {
            return;
        }
        if !self.allow_remote {
            self.blocked.fetch_add(1, Ordering::SeqCst);
            return;
        }
        if self.started.fetch_add(1, Ordering::SeqCst) >= MAX_FETCHES {
            return;
        }
        let pending = self.pending.clone();
        pending.fetch_add(1, Ordering::SeqCst);
        std::thread::spawn(move || {
            if let Some(data) = fetch(&url) {
                handler.bytes(url, Bytes::from(data));
            }
            pending.fetch_sub(1, Ordering::SeqCst);
        });
    }
}

fn fetch(url: &str) -> Option<Vec<u8>> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(FETCH_DEADLINE))
        .build()
        .into();
    let mut response = agent.get(url).call().ok()?;
    response
        .body_mut()
        .with_config()
        .limit(MAX_RESOURCE_BYTES)
        .read_to_vec()
        .ok()
}

/// Text the message does not style gets the app's font (Inter), as other
/// mail apps give it theirs, instead of the browser default (a serif).
/// The message's own styles still win over this.
const DEFAULT_STYLE: &str = "body { font-family: Inter, sans-serif; }";

/// System fonts plus the fonts bundled with the app.
fn fonts() -> parley::FontContext {
    static FONTS: OnceLock<parley::FontContext> = OnceLock::new();
    FONTS
        .get_or_init(|| {
            let mut ctx = parley::FontContext::new();
            for font in crate::ui::FONTS {
                ctx.collection
                    .register_fonts(parley::fontique::Blob::new(Arc::new(font)), None);
            }
            ctx
        })
        .clone()
}

/// Parses the message and waits (bounded) for its images and style sheets.
fn load(job: &Job) -> Loaded {
    let scale = job.scale.clamp(1.0, 3.0);
    let width = job.width.max(200);
    let pending = Arc::new(AtomicUsize::new(0));
    let blocked = Arc::new(AtomicUsize::new(0));
    let net = Net {
        allow_remote: job.allow_remote,
        inline: job
            .inline
            .iter()
            .map(|(k, v)| (k.clone(), Bytes::from(v.clone())))
            .collect(),
        pending: pending.clone(),
        started: AtomicUsize::new(0),
        blocked: blocked.clone(),
    };
    let physical_w = (width as f32 * scale).round() as u32;
    let config = DocumentConfig {
        viewport: Some(Viewport::new(
            physical_w,
            (800.0 * scale) as u32,
            scale,
            ColorScheme::Light,
        )),
        net_provider: Some(Arc::new(net)),
        font_ctx: Some(fonts()),
        ua_stylesheets: vec![
            blitz_dom::UserAgentStylesheet::default_sheet(),
            blitz_dom::UserAgentStylesheet::parse(DEFAULT_STYLE),
        ],
        ..Default::default()
    };
    // A text/html part is HTML whatever it declares: Blitz parses anything
    // starting with an XHTML doctype or `<html xmlns=…xhtml>` as XML, where
    // the HTML user-agent styles do not apply (head, title and style text
    // show up, paragraphs and tables collapse). A leading comment keeps its
    // sniffing off and is harmless to the HTML parser.
    let html = format!("<!---->{}", job.html);
    let mut doc = HtmlDocument::from_html(&html, config);
    doc.resolve(0.0);
    // Wait for images/stylesheets (bounded), then lay out again.
    let deadline = Instant::now() + FETCH_DEADLINE;
    while pending.load(Ordering::SeqCst) > 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(15));
    }
    doc.resolve(0.0);
    Loaded {
        uid: job.uid,
        allow_remote: job.allow_remote,
        html_len: job.html.len(),
        doc,
        blocked_remote: blocked.load(Ordering::SeqCst) > 0,
        painted: None,
    }
}

/// Lays the loaded message out at `width` and paints it into tiles.
fn paint(loaded: &mut Loaded, width: u32, scale: f32) -> Rendered {
    let scale = scale.clamp(1.0, 3.0);
    let width = width.max(200);
    let doc = &mut loaded.doc;
    let layout = |doc: &mut HtmlDocument, w: u32| {
        let physical_w = (w as f32 * scale).round() as u32;
        doc.set_viewport(Viewport::new(
            physical_w,
            (800.0 * scale) as u32,
            scale,
            ColorScheme::Light,
        ));
        doc.set_viewport_scroll(blitz_dom::Point { x: 0.0, y: 0.0 });
        doc.resolve(0.0);
        physical_w
    };
    let mut physical_w = layout(doc, width);
    // Content wider than the room (a 600 px table): lay it out at its own
    // width and show it shrunk, instead of cutting its right side.
    let content_w = (doc.root_element().scrollable_overflow().x1 / scale as f64).ceil() as u32;
    let mut zoom = 1.0;
    if content_w > width + 1 {
        let laid = content_w.min(width * 3);
        physical_w = layout(doc, laid);
        zoom = width as f32 / laid as f32;
    }

    // The content's height, not the root box's: with `html, body
    // { height: 100% }` the root is as tall as the viewport and the rest
    // overflows it.
    let root = doc.root_element();
    let content = (root.scrollable_overflow().y1 / scale as f64) as f32;
    let height = (root.final_layout().size.height.max(content).ceil() as u32).clamp(1, MAX_HEIGHT);
    let physical_h = (height as f32 * scale).ceil() as u32;
    let painted = Painted {
        width,
        zoom,
        scale,
        physical_w,
        physical_h,
    };
    loaded.painted = Some(painted);
    tiles(loaded, painted)
}

/// Paints the message as last laid out again (after a selection change).
fn repaint(loaded: &mut Loaded) -> Option<Rendered> {
    let painted = loaded.painted?;
    Some(tiles(loaded, painted))
}

/// Paints the laid-out message into tiles.
fn tiles(loaded: &mut Loaded, painted: Painted) -> Rendered {
    let Painted {
        width,
        zoom,
        scale,
        physical_w,
        physical_h,
    } = painted;
    let doc = &mut loaded.doc;
    // Paint tile by tile: a viewport as tall as the tile, scrolled to it.
    let mut tiles = Vec::new();
    let mut y = 0;
    while y < physical_h {
        let h = TILE.min(physical_h - y);
        doc.set_viewport(Viewport::new(physical_w, h, scale, ColorScheme::Light));
        doc.resolve(0.0);
        doc.set_viewport_scroll(blitz_dom::Point {
            x: 0.0,
            y: y as f64 / scale as f64,
        });
        let pixels = render_to_buffer::<VelloCpuImageRenderer, _>(
            |scene| blitz_paint::paint_scene(scene, doc, scale as f64, physical_w, h, 0, 0),
            physical_w,
            h,
        );
        tiles.push((image::Handle::from_rgba(physical_w, h, pixels), h as f32 / scale));
        y += h;
    }
    // Full-height, unscrolled viewport so hit testing works anywhere.
    doc.set_viewport(Viewport::new(physical_w, physical_h, scale, ColorScheme::Light));
    doc.resolve(0.0);
    doc.set_viewport_scroll(blitz_dom::Point { x: 0.0, y: 0.0 });

    Rendered {
        uid: loaded.uid,
        width,
        zoom,
        tiles,
        blocked_remote: loaded.blocked_remote,
        selection: doc.has_text_selection(),
    }
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let (Some(h), Some(l)) = (
                bytes.get(i + 1).and_then(|b| (*b as char).to_digit(16)),
                bytes.get(i + 2).and_then(|b| (*b as char).to_digit(16)),
            )
        {
            out.push((h * 16 + l) as u8);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The renderer keeps only the latest request: tests take turns.
    static SERIAL: Mutex<()> = Mutex::new(());

    #[test]
    fn renders_tiles_and_finds_links() {
        let _turn = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let html = r#"<html><body style="margin:0">
            <div style="height:3000px;background:#eee">
              <a href="https://ferrowl.com/x" style="display:block;height:100px">link</a>
            </div>
            <img src="cid:logo@x" width="10" height="10">
            <img src="https://tracker.invalid/p.gif">
        </body></html>"#;
        let mut inline = HashMap::new();
        inline.insert("logo@x".to_string(), vec![0u8; 4]);
        let r = render(Job {
            uid: 7,
            html: html.into(),
            width: 600,
            scale: 1.0,
            allow_remote: false,
            inline,
        })
        .unwrap();
        assert_eq!(r.width, 600);
        assert!(r.tiles.len() >= 2, "3000px+ must be split into tiles");
        let total: f32 = r.tiles.iter().map(|(_, h)| h).sum();
        assert!(total >= 3000.0);
        assert!(r.blocked_remote, "remote image must be blocked");
        assert_eq!(link_at(7, 50.0, 50.0).as_deref(), Some("https://ferrowl.com/x"));
        assert_eq!(link_at(7, 50.0, 2000.0), None);
        assert_eq!(link_at(8, 50.0, 50.0), None, "other message");
    }

    #[test]
    fn drag_and_double_click_select_text() {
        let _turn = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let html = r#"<html><body style="margin:0;font:16px sans-serif">
            <p style="margin:0">Your order code is ABC-123.</p>
            <p style="margin:0">Second line here</p>
            <div style="height:5000px"></div>
        </body></html>"#;
        let r = render(Job {
            uid: 9,
            html: html.into(),
            width: 600,
            scale: 2.0,
            allow_remote: false,
            inline: HashMap::new(),
        })
        .unwrap();
        assert!(!r.selection);
        assert!(
            select(9, Select::Start(1.0, 8.0)).is_none(),
            "an empty selection paints nothing"
        );
        let started = Instant::now();
        let r = select(9, Select::Extend(590.0, 28.0)).expect("drag repaints");
        eprintln!("repaint of {} tiles: {:?}", r.tiles.len(), started.elapsed());
        assert!(r.selection);
        let text = selected_text(9).unwrap();
        assert!(text.starts_with("Your order code is ABC-123."), "{text:?}");
        assert!(text.contains("Second line"), "{text:?}");
        assert!(select(10, Select::Start(1.0, 8.0)).is_none(), "other message");
        // Double click on the code takes it whole.
        let x = (0..600).map(|x| x as f32).find(|&x| {
            select(9, Select::Word(x, 8.0));
            selected_text(9).as_deref() == Some("ABC-123")
        });
        assert!(x.is_some(), "double click selects ABC-123");
    }

    #[test]
    fn words_stop_at_spaces_and_sentence_ends() {
        let t = "Code: ABC-123. Mail fabio@ferrowl.com now";
        let at = |w: &str| {
            let i = t.find(w).unwrap() + 1;
            let (a, b) = word_at(t, i);
            &t[a..b]
        };
        assert_eq!(at("ABC"), "ABC-123");
        assert_eq!(at("fabio"), "fabio@ferrowl.com");
        assert_eq!(at("now"), "now");
        assert_eq!(word_at(t, t.len()), (t.len() - 3, t.len()));
    }

    #[test]
    fn xhtml_doctype_is_parsed_as_html() {
        let _turn = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        for head in [
            "<!DOCTYPE html PUBLIC \"-//W3C//DTD XHTML 1.0 Transitional//EN\"\n\"http://www.w3.org/TR/xhtml1/DTD/xhtml1-transitional.dtd\">\n<html>",
            "<html xmlns=\"http://www.w3.org/1999/xhtml\">",
        ] {
            let html = format!(
                "{head}<head><title>T</title><style>div {{ height: 500px }}</style></head>\
                 <body style=\"margin:0\"><div>a</div></body></html>"
            );
            let r = render(Job {
                uid: 9,
                html,
                width: 600,
                scale: 1.0,
                allow_remote: false,
                inline: HashMap::new(),
            })
            .unwrap();
            let total: f32 = r.tiles.iter().map(|(_, h)| h).sum();
            assert!((500.0..520.0).contains(&total), "style must apply, got {total}");
        }
    }

    #[test]
    fn full_height_body_keeps_its_overflow() {
        let _turn = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let html = r#"<html style="height:100%"><body style="height:100%;margin:0">
            <div style="height:2000px">top</div><p>footer</p></body></html>"#;
        let job = |width| Job {
            uid: 11,
            html: html.into(),
            width,
            scale: 1.0,
            allow_remote: false,
            inline: HashMap::new(),
        };
        let total = |r: Rendered| r.tiles.iter().map(|(_, h)| h).sum::<f32>();
        assert!(
            total(render(job(600)).unwrap()) > 2000.0,
            "footer cut at the viewport"
        );
        // Same message, new width: laid out again from the kept document.
        let r = render(job(400)).unwrap();
        assert_eq!(r.width, 400);
        assert!(total(r) > 2000.0);
    }

    /// Renders `$FERROWL_HTML` and writes its tiles next to it (manual check).
    #[test]
    #[ignore = "manual: needs FERROWL_HTML"]
    fn dump_tiles() {
        let _turn = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let path = std::path::PathBuf::from(std::env::var("FERROWL_HTML").unwrap());
        let html = std::fs::read_to_string(&path).unwrap();
        let t = Instant::now();
        let job = |width| Job {
            uid: 1,
            html: html.clone(),
            width,
            scale: 1.0,
            allow_remote: true,
            inline: HashMap::new(),
        };
        render(job(500)).unwrap();
        println!("first render {:?}", t.elapsed());
        let t = Instant::now();
        render(job(600)).unwrap();
        println!("re-render at a new width {:?}", t.elapsed());
        let r = render(Job {
            uid: 1,
            html,
            width: 700,
            scale: 1.0,
            allow_remote: true,
            inline: HashMap::new(),
        })
        .unwrap();
        for (i, (handle, h)) in r.tiles.iter().enumerate() {
            let iced::advanced::image::Handle::Rgba {
                width,
                height,
                pixels,
                ..
            } = handle
            else {
                panic!()
            };
            let f = std::fs::File::create(path.with_extension(format!("tile{i}.png"))).unwrap();
            let mut e = png::Encoder::new(f, *width, *height);
            e.set_color(png::ColorType::Rgba);
            e.write_header().unwrap().write_image_data(pixels).unwrap();
            println!("tile {i}: {width}x{height} logical h {h}");
        }
    }
}
