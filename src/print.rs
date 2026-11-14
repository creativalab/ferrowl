//! Printing a message: a page with the message's headers and body is
//! written to a private temporary file and opened in the default browser,
//! which shows its print dialog right away (the browser prints well and
//! knows the system's printers).
//!
//! The mail's own HTML is kept, but a Content-Security-Policy stops its
//! scripts; only our one-line `print()` runs (allowed by a random nonce).
//! What the policy does not cover (`<meta http-equiv=refresh>` redirects,
//! `<base>`, forms, linked style sheets) is taken out of the mail first.
//! Remote images load only if the user allows them for mail.

use crate::mail::FullMessage;
use base64::Engine;
use std::path::PathBuf;

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The page to print.
pub fn page(msg: &FullMessage, remote_images: bool, nonce: &str, labels: &Labels) -> String {
    let img_src = if remote_images {
        "data: https: http:"
    } else {
        "data:"
    };
    let body = match &msg.html {
        Some(html) => {
            // Inline images (cid:) become data: URLs, so they print too.
            let mut html = html.clone();
            for (id, data) in &msg.inline {
                let b64 = base64::engine::general_purpose::STANDARD.encode(data);
                let mime = image_mime(data);
                html = html.replace(&format!("cid:{id}"), &format!("data:{mime};base64,{b64}"));
            }
            format!("<div class=\"mail\">{}</div>", without_active_tags(&html))
        }
        None => format!("<pre class=\"mail\">{}</pre>", escape(&msg.body)),
    };
    let mut head = format!(
        "<tr><th>{}</th><td>{} &lt;{}&gt;</td></tr>",
        escape(labels.from),
        escape(&msg.from),
        escape(&msg.from_address)
    );
    head += &format!(
        "<tr><th>{}</th><td>{}</td></tr>",
        escape(labels.to),
        escape(&msg.to)
    );
    if !msg.cc.is_empty() {
        head += &format!("<tr><th>Cc</th><td>{}</td></tr>", escape(&msg.cc));
    }
    head += &format!(
        "<tr><th>{}</th><td>{}</td></tr>",
        escape(labels.date),
        escape(&labels.when)
    );
    format!(
        r#"<!doctype html>
<html><head><meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src {img_src}; style-src 'unsafe-inline'; font-src data:; script-src 'nonce-{nonce}'; form-action 'none'; base-uri 'none'">
<title>{title}</title>
<style>
body {{ font: 14px/1.5 system-ui, sans-serif; color: #111; margin: 24px; }}
h1 {{ font-size: 20px; margin: 0 0 12px; }}
table.head {{ border-collapse: collapse; margin-bottom: 16px; }}
table.head th {{ text-align: left; color: #555; font-weight: 600; padding: 2px 12px 2px 0; vertical-align: top; }}
hr {{ border: 0; border-top: 1px solid #ccc; margin: 0 0 16px; }}
pre.mail {{ white-space: pre-wrap; font: inherit; margin: 0; }}
.mail img {{ max-width: 100%; height: auto; }}
</style>
<script nonce="{nonce}">window.addEventListener("load", function () {{ window.print(); }});</script>
</head><body>
<h1>{title}</h1>
<table class="head">{head}</table>
<hr>
{body}
</body></html>
"#,
        title = escape(&msg.subject),
    )
}

/// The mail's HTML without the tags that act on their own when the page
/// opens: `<meta>` (a refresh navigates away, to a tracker or worse),
/// `<base>`, `<link>`, `<form>` and scripts (with their content).
fn without_active_tags(html: &str) -> String {
    // ASCII lowercase keeps every byte where it is: indices match `html`.
    let lower = html.to_ascii_lowercase();
    let mut out = String::with_capacity(html.len());
    let mut i = 0;
    while let Some(rel) = lower[i..].find('<') {
        let start = i + rel;
        out.push_str(&html[i..start]);
        let after = &lower[start + 1..];
        let closing = after.starts_with('/');
        let name: String = after
            .trim_start_matches('/')
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect();
        let end = lower[start..].find('>').map_or(html.len(), |e| start + e + 1);
        match name.as_str() {
            // The script's text goes too, up to its end tag.
            "script" if !closing => {
                i = lower[end..]
                    .find("</script")
                    .and_then(|c| lower[end + c..].find('>').map(|e| end + c + e + 1))
                    .unwrap_or(html.len());
            }
            "script" | "meta" | "base" | "link" | "form" => i = end,
            _ => {
                out.push_str(&html[start..end]);
                i = end;
            }
        }
    }
    out.push_str(&html[i..]);
    out
}

/// Image type from the first bytes (inline images are almost always one
/// of these).
fn image_mime(data: &[u8]) -> &'static str {
    match data {
        [0x89, b'P', b'N', b'G', ..] => "image/png",
        [0xFF, 0xD8, 0xFF, ..] => "image/jpeg",
        [b'G', b'I', b'F', b'8', ..] => "image/gif",
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => "image/webp",
        _ => "application/octet-stream",
    }
}

/// Translated header labels and the formatted date.
pub struct Labels {
    pub from: &'static str,
    pub to: &'static str,
    pub date: &'static str,
    pub when: String,
}

/// How long the page stays on disk: the browser reads it right away.
const PAGE_LIFETIME: std::time::Duration = std::time::Duration::from_secs(10 * 60);

/// Writes the page (owner-only file) and opens it in the browser.
pub fn open(msg: &FullMessage, remote_images: bool, labels: &Labels) -> anyhow::Result<()> {
    let nonce: String = {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        (0..24).map(|_| char::from(b'a' + rng.gen_range(0..26))).collect()
    };
    let html = page(msg, remote_images, &nonce, labels);
    // A new file with a random name, owner-only, created exclusively: on a
    // shared /tmp nobody can have put a link or a file there in advance.
    let mut file = tempfile::Builder::new()
        .prefix("ferrowl-print-")
        .suffix(".html")
        .tempfile()?;
    {
        use std::io::Write;
        file.write_all(html.as_bytes())?;
    }
    let (_, path): (_, PathBuf) = file.keep()?;
    open::that_detached(&path)?;
    std::thread::spawn(move || {
        std::thread::sleep(PAGE_LIFETIME);
        let _ = std::fs::remove_file(path);
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels() -> Labels {
        Labels {
            from: "Da",
            to: "A",
            date: "Data",
            when: "3 nov 2026".into(),
        }
    }

    #[test]
    fn text_mail_is_escaped_and_scripts_are_blocked() {
        let msg = FullMessage {
            uid: 1,
            from: "Ada <script>".into(),
            from_address: "ada@x.it".into(),
            to: "me@x.it".into(),
            cc: String::new(),
            to_addrs: vec![],
            cc_addrs: vec![],
            subject: "Hi & bye".into(),
            timestamp: 0,
            message_id: None,
            references: vec![],
            body: "line <b>1</b>".into(),
            html: None,
            inline: Default::default(),
            attachments: vec![],
            dkim_pass: false,
            pgp: None,
            calendar: None,
        };
        let page = page(&msg, false, "abc", &labels());
        assert!(page.contains("script-src 'nonce-abc'"));
        assert!(page.contains("img-src data:;"), "no remote images");
        assert!(page.contains("Ada &lt;script&gt;"));
        assert!(page.contains("line &lt;b&gt;1&lt;/b&gt;"));
        assert!(page.contains("<title>Hi &amp; bye</title>"));
    }

    #[test]
    fn active_tags_are_taken_out_of_mail_html() {
        let html = r#"<META http-equiv="refresh" content="0;url=https://t.example"><base href="https://x">
<p>Hi <b>there</b></p><link rel=stylesheet href="https://s"><form action="https://f"><input></form>
<script>alert("<b>")</script><SCRIPT src=x></SCRIPT><metadata>ok</metadata>è"#;
        let out = without_active_tags(html);
        assert!(!out.to_lowercase().contains("<meta "), "{out}");
        assert!(!out.contains("refresh") && !out.contains("<base") && !out.contains("<link"));
        assert!(!out.contains("<form") && !out.contains("</form>") && !out.contains("alert"));
        assert!(!out.to_lowercase().contains("script"));
        assert!(out.contains("<p>Hi <b>there</b></p>") && out.contains("<input>"));
        assert!(out.contains("<metadata>ok</metadata>è"));
    }
}
