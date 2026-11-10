//! What a message is about, picked out of its text so the apps can act on
//! it: a shipment (carrier, tracking number, the page to follow it), a
//! verification code to copy, a bill to pay (number, amount, due date).
//!
//! Careful rather than complete: something is offered only when the words
//! around it say what it is ("spedizione", "codice", "fattura"…), so a
//! random number in a newsletter is not taken for one.

use std::sync::LazyLock;

use chrono::NaiveDate;
use regex::Regex;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Insight {
    /// A parcel on its way. `url` follows it (empty when the carrier is unknown).
    Shipment {
        carrier: String,
        number: String,
        url: String,
    },
    /// A one-time code (login, payment confirmation).
    Code { code: String },
    /// A bill: its number (may be empty), amount as written ("€ 123,45";
    /// may be empty) and due date.
    Payment {
        number: String,
        amount: String,
        due: Option<NaiveDate>,
    },
}

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).expect("valid pattern")
}

static SHIPPING_WORDS: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"(?i)\b(spedizion\w*|spedit\w*|consegn\w*|tracking|tracciament\w*|pacc[oh]\w*|corriere|shipped|shipment|delivery|parcel|lettera di vettura|LDV)\b",
    )
});
static CODE: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"(?i)\b(codice|code|otp|pin|passcode|verifica|verification|one[- ]time|sicurezza|security)\b[^\d\n]{0,40}?\b(\d{4,8})\b",
    )
});
static BILL_WORDS: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"(?i)\b(fattur\w*|invoice|bollett\w*|pagament\w*|payment|scadenz\w*|rata|f24|avviso di pagamento|importo|amount due|da pagare)\b",
    )
});
static INVOICE_NUMBER: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"(?i)\b(?:fattura|invoice)\s*(?:n\.?|nr\.?|no\.?|num(?:ero)?\.?|#)?\s*[:°]?\s*([A-Z0-9][A-Z0-9/\-]{0,19})",
    )
});
static AMOUNT_NEAR: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"(?i)\b(?:totale|importo|da pagare|total|amount due|amount)\b[^\d€\n]{0,30}((?:€|EUR)\s?\d[\d.\s]*,\d{2}|(?:€|EUR)\s?\d+(?:\.\d{2})?|\d[\d.]*,\d{2}\s?(?:€|EUR))",
    )
});
static AMOUNT: LazyLock<Regex> = LazyLock::new(|| {
    re(r"(?i)((?:€|EUR)\s?\d[\d.]*,\d{2}|(?:€|EUR)\s?\d+\.\d{2}|\d[\d.]*,\d{2}\s?(?:€|EUR))")
});
static DUE_NUMERIC: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"(?i)\b(?:scadenza|scade|entro il|entro|da pagare entro|due date|due by|due on|payment due)\b\D{0,20}?(\d{1,2})[/.\-](\d{1,2})[/.\-](\d{2,4})",
    )
});
static DUE_WORDS: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"(?i)\b(?:scadenza|scade|entro il|entro|da pagare entro|due date|due by|due on|payment due)\b\D{0,20}?(\d{1,2})\s+([a-zà]+)\s+(\d{4})",
    )
});

/// What `subject` and `text` (the plain body) of a message from `from`
/// (its address) are about, at most one of each kind.
pub fn find(subject: &str, from: &str, text: &str) -> Vec<Insight> {
    let all = format!("{subject}\n{text}");
    let mut out = Vec::new();
    if let Some(s) = shipment(&all, from) {
        out.push(s);
    }
    if let Some(c) = code(&all) {
        out.push(c);
    }
    if let Some(p) = payment(&all) {
        out.push(p);
    }
    out
}

/// Known carriers: (name, words that name it, the number's pattern, page).
struct Carrier {
    name: &'static str,
    words: &'static [&'static str],
    number: &'static str,
    url: &'static str,
}

const CARRIERS: &[Carrier] = &[
    Carrier {
        name: "UPS",
        words: &["ups"],
        number: r"\b(1Z[0-9A-Z]{16})\b",
        url: "https://www.ups.com/track?tracknum={}",
    },
    Carrier {
        name: "Amazon",
        words: &["amazon"],
        number: r"\b(TBA\d{12})\b",
        url: "https://track.amazon.it/tracking/{}",
    },
    Carrier {
        name: "Poste Italiane",
        words: &["poste", "posteitaliane", "poste delivery"],
        number: r"\b([A-Z]{2}\d{9}IT|\d{12,13})\b",
        url: "https://www.poste.it/cerca/index.html#/risultati-spedizioni/{}",
    },
    Carrier {
        name: "DHL",
        words: &["dhl"],
        number: r"\b(J?JD\d{18}|\d{10})\b",
        url: "https://www.dhl.com/it-it/home/tracking.html?tracking-id={}",
    },
    Carrier {
        name: "FedEx",
        words: &["fedex"],
        number: r"\b(\d{12})\b",
        url: "https://www.fedex.com/fedextrack/?trknbr={}",
    },
    Carrier {
        name: "BRT",
        words: &["brt", "bartolini"],
        number: r"\b(\d{10,15})\b",
        url: "https://vas.brt.it/vas/sped_det_show.hsm?referer=sped_numspe_par.htm&Nspediz={}",
    },
    Carrier {
        name: "GLS",
        words: &["gls"],
        number: r"\b([A-Z0-9]{8,14})\b",
        url: "https://gls-group.com/IT/it/servizi-online/ricerca-spedizioni?match={}",
    },
    Carrier {
        name: "SDA",
        words: &["sda"],
        number: r"\b([A-Z0-9]{13})\b",
        url: "https://www.sda.it/wps/portal/Servizi_online/ricerca_spedizioni?locale=it&tracing.letteraVettura={}",
    },
];

static GENERIC_TRACKING: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"(?i)\b(?:tracking(?: number| code)?|numero (?:di )?(?:spedizione|tracking|tracciamento)|codice (?:di )?(?:spedizione|tracciamento)|lettera di vettura|LDV)\b\W{0,5}([A-Z0-9]{8,30})",
    )
});

fn shipment(all: &str, from: &str) -> Option<Insight> {
    if !SHIPPING_WORDS.is_match(all) {
        return None;
    }
    let lower = format!("{} {}", all.to_lowercase(), from.to_lowercase());
    let named = |words: &[&str]| {
        words.iter().any(|w| {
            Regex::new(&format!(r"\b{}\b", regex::escape(w)))
                .map(|r| r.is_match(&lower))
                .unwrap_or(false)
        })
    };
    // Unmistakable numbers (UPS, Amazon) wherever they are; the others
    // only when the carrier is named.
    for c in CARRIERS {
        let distinctive = matches!(c.name, "UPS" | "Amazon");
        if !distinctive && !named(c.words) {
            continue;
        }
        // Prefer the number right after a tracking word, then any match.
        let pattern = re(c.number);
        let after_word = GENERIC_TRACKING
            .captures_iter(all)
            .filter_map(|cap| cap.get(1))
            .map(|m| m.as_str().to_uppercase())
            .find(|n| pattern.is_match(n));
        let number = after_word.or_else(|| {
            pattern
                .captures(all)
                .and_then(|cap| cap.get(1))
                .map(|m| m.as_str().to_uppercase())
                .filter(|n| distinctive || n.chars().any(|ch| ch.is_ascii_digit()))
        });
        if let Some(number) = number {
            return Some(Insight::Shipment {
                carrier: c.name.to_string(),
                url: c.url.replace("{}", &number),
                number,
            });
        }
    }
    // An unknown carrier: the number right after "tracking" & co.
    GENERIC_TRACKING
        .captures_iter(all)
        .filter_map(|cap| cap.get(1).map(|m| m.as_str().to_string()))
        .find(|n| n.chars().filter(char::is_ascii_digit).count() >= 4)
        .map(|number| Insight::Shipment {
            carrier: String::new(),
            number,
            url: String::new(),
        })
}

fn code(all: &str) -> Option<Insight> {
    CODE.captures_iter(all)
        .filter_map(|cap| cap.get(2).map(|m| m.as_str().to_string()))
        // A year is not a code.
        .find(|c| !(c.len() == 4 && (c.starts_with("19") || c.starts_with("20"))))
        .map(|code| Insight::Code { code })
}

fn payment(all: &str) -> Option<Insight> {
    if !BILL_WORDS.is_match(all) {
        return None;
    }
    let amount = AMOUNT_NEAR
        .captures(all)
        .and_then(|c| c.get(1))
        .or_else(|| AMOUNT.captures(all).and_then(|c| c.get(1)))
        .map(|m| m.as_str().split_whitespace().collect::<Vec<_>>().join(" "))
        .unwrap_or_default();
    let due = due_date(all);
    if amount.is_empty() && due.is_none() {
        return None;
    }
    let number = INVOICE_NUMBER
        .captures(all)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
        .filter(|n| n.chars().any(|ch| ch.is_ascii_digit()))
        .unwrap_or_default();
    Some(Insight::Payment { number, amount, due })
}

fn due_date(all: &str) -> Option<NaiveDate> {
    let year = |y: &str| -> Option<i32> {
        let y: i32 = y.parse().ok()?;
        Some(if y < 100 { 2000 + y } else { y })
    };
    if let Some(c) = DUE_NUMERIC.captures(all) {
        // Day first, as written in Europe.
        let (d, m, y) = (c[1].parse().ok()?, c[2].parse().ok()?, year(&c[3])?);
        if let Some(date) = NaiveDate::from_ymd_opt(y, m, d) {
            return Some(date);
        }
    }
    let c = DUE_WORDS.captures(all)?;
    let month = month_number(&c[2].to_lowercase())?;
    NaiveDate::from_ymd_opt(year(&c[3])?, month, c[1].parse().ok()?)
}

fn month_number(name: &str) -> Option<u32> {
    const NAMES: [&[&str]; 12] = [
        &["gennaio", "january", "jan", "gen"],
        &["febbraio", "february", "feb"],
        &["marzo", "march", "mar"],
        &["aprile", "april", "apr"],
        &["maggio", "may", "mag"],
        &["giugno", "june", "jun", "giu"],
        &["luglio", "july", "jul", "lug"],
        &["agosto", "august", "aug", "ago"],
        &["settembre", "september", "sep", "sept", "set"],
        &["ottobre", "october", "oct", "ott"],
        &["novembre", "november", "nov"],
        &["dicembre", "december", "dec", "dic"],
    ];
    NAMES
        .iter()
        .position(|names| names.contains(&name))
        .map(|i| i as u32 + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipments() {
        let ups = find(
            "Il tuo ordine è stato spedito",
            "noreply@shop.it",
            "Tracking: 1Z999AA10123456784",
        );
        assert_eq!(
            ups,
            [Insight::Shipment {
                carrier: "UPS".into(),
                number: "1Z999AA10123456784".into(),
                url: "https://www.ups.com/track?tracknum=1Z999AA10123456784".into(),
            }]
        );
        let poste = find(
            "Spedizione in consegna",
            "noreply@posteitaliane.it",
            "Il pacco RR123456789IT arriverà domani.",
        );
        assert!(
            matches!(&poste[0], Insight::Shipment { carrier, number, .. } if carrier == "Poste Italiane" && number == "RR123456789IT")
        );
        let brt = find(
            "BRT - la tua spedizione",
            "noreply@brt.it",
            "Numero di spedizione: 123456789012",
        );
        assert!(
            matches!(&brt[0], Insight::Shipment { carrier, number, .. } if carrier == "BRT" && number == "123456789012")
        );
        // A number in a newsletter without shipping words is not a parcel.
        assert!(find("Offerte DHL", "news@x.it", "Chiamaci allo 0612345678").is_empty());
    }

    #[test]
    fn codes() {
        assert_eq!(
            find(
                "Il tuo codice",
                "",
                "Il codice di verifica è 482913. Scade tra 10 minuti."
            ),
            [Insight::Code {
                code: "482913".into()
            }]
        );
        assert_eq!(
            find("Login", "", "Your verification code: 7714"),
            [Insight::Code { code: "7714".into() }]
        );
        // A year after "code" is not a code.
        assert!(find("Code of conduct 2026", "", "Aggiornato per il 2026").is_empty());
    }

    #[test]
    fn payments() {
        let p = find(
            "Fattura n. 2026/145",
            "",
            "Gentile cliente, in allegato la fattura. Totale da pagare: € 1.234,50 con scadenza 30/11/2026.",
        );
        assert_eq!(
            p,
            [Insight::Payment {
                number: "2026/145".into(),
                amount: "€ 1.234,50".into(),
                due: NaiveDate::from_ymd_opt(2026, 11, 30),
            }]
        );
        let words = find(
            "Bolletta luce",
            "",
            "Importo 89,90 € - da pagare entro il 15 dicembre 2026",
        );
        assert!(matches!(&words[0], Insight::Payment { amount, due, .. }
            if amount == "89,90 €" && *due == NaiveDate::from_ymd_opt(2026, 12, 15)));
        // Prices in an ad are not a bill.
        assert!(find("Saldi!", "", "Scarpe a 49,90 €, borse a 29,90 €").is_empty());
    }
}
