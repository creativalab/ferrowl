#!/usr/bin/env python3
"""Makes the Google Play phone screenshots (1080x1920, 9:16) from the raw
captures of store/tools/capture_android.sh: a headline in the store's
language over a soft orange background, the capture below it (the same
look as the App Store ones, store/tools/compose_ios.swift).

    python3 store/tools/compose_android.py [store/android] [contact-sheet-dir]

Reads <locale>/raw/NN_scene.png, writes <locale>/NN_scene.png. With a
second argument, one small sheet per locale with all its images goes there
(to check them at a glance). Needs Pillow with Raqm (Arabic shaping) and the
Noto fonts (fonts-noto-core, fonts-noto-cjk).
"""
import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter, ImageFont

SCENES = ["01_inbox", "02_message", "03_undo", "04_accounts", "05_invite", "06_calendar", "07_pgp"]

# Headlines per Play locale, by scene (the App Store ones where the scene is
# the same). Lines broken by hand (Chinese, Japanese, Korean) stay as they are.
HEADLINES = {
    "en-US": {
        "inbox": "Email that doesn't track you", "message": "Senders never know when you read",
        "undo": "Undo with one tap", "accounts": "All your mailboxes in one app",
        "invite": "Answer invitations right in the mail", "calendar": "Your calendar, built in",
        "pgp": "OpenPGP signing and encryption",
    },
    "it-IT": {
        "inbox": "La posta che non ti traccia", "message": "Chi scrive non sa quando leggi",
        "undo": "Annulla con un tocco", "accounts": "Tutte le tue caselle in un'app",
        "invite": "Rispondi agli inviti direttamente dalla mail", "calendar": "Il calendario, già incluso",
        "pgp": "Firma e cifratura OpenPGP",
    },
    "es-ES": {
        "inbox": "Correo que no te rastrea", "message": "Nadie sabe cuándo lees",
        "undo": "Deshaz con un toque", "accounts": "Todos tus buzones en una app",
        "invite": "Responde a invitaciones desde el correo", "calendar": "Tu calendario, integrado",
        "pgp": "Firma y cifrado OpenPGP",
    },
    "fr-FR": {
        "inbox": "Des e-mails qui ne vous pistent pas", "message": "Personne ne sait quand vous lisez",
        "undo": "Annulez d'un geste", "accounts": "Toutes vos boîtes dans une app",
        "invite": "Répondez aux invitations depuis l'e-mail", "calendar": "Votre agenda, intégré",
        "pgp": "Signature et chiffrement OpenPGP",
    },
    "de-DE": {
        "inbox": "E-Mail, die dich nicht verfolgt", "message": "Absender sehen nicht, wann du liest",
        "undo": "Mit einem Tipp rückgängig", "accounts": "Alle Postfächer in einer App",
        "invite": "Einladungen direkt in der Mail beantworten", "calendar": "Dein Kalender, direkt dabei",
        "pgp": "Signieren und verschlüsseln mit OpenPGP",
    },
    "pt-BR": {
        "inbox": "E-mail que não rastreia você", "message": "Ninguém sabe quando você lê",
        "undo": "Desfaça com um toque", "accounts": "Todas as suas caixas em um app",
        "invite": "Responda a convites direto no e-mail", "calendar": "Sua agenda, integrada",
        "pgp": "Assinatura e criptografia OpenPGP",
    },
    "ja-JP": {
        "inbox": "あなたを追跡しない\nメール", "message": "いつ読んだか、\n送信者にはわかりません",
        "undo": "ワンタップで取り消し", "accounts": "すべてのメールを\n1つのアプリで",
        "invite": "招待にはメールから\nそのまま返信", "calendar": "カレンダーも内蔵",
        "pgp": "OpenPGPで\n署名と暗号化",
    },
    "ko-KR": {
        "inbox": "나를 추적하지 않는 이메일", "message": "읽은 시점을\n보낸 사람은 알 수 없어요",
        "undo": "한 번의 탭으로 실행 취소", "accounts": "모든 메일함을\n하나의 앱에서",
        "invite": "초대에 메일에서\n바로 응답하세요", "calendar": "캘린더도 기본 탑재",
        "pgp": "OpenPGP 서명과 암호화",
    },
    "zh-CN": {
        "inbox": "不追踪你的邮件", "message": "发件人无从知道\n你何时阅读",
        "undo": "轻点一下即可撤销", "accounts": "所有邮箱，尽在一个应用",
        "invite": "在邮件中\n直接回复邀请", "calendar": "内置日历",
        "pgp": "OpenPGP 签名与加密",
    },
    "ar": {
        "inbox": "بريد لا يتتبّعك", "message": "لا يعرف المرسِل متى تقرأ",
        "undo": "تراجع بلمسة واحدة", "accounts": "كل صناديق بريدك في تطبيق واحد",
        "invite": "رُدّ على الدعوات من الرسالة مباشرة", "calendar": "تقويمك مدمج في التطبيق",
        "pgp": "توقيع وتشفير OpenPGP",
    },
}

NOTO = Path("/usr/share/fonts/truetype/noto")
CJK = Path("/usr/share/fonts/opentype/noto/NotoSansCJK-Bold.ttc")


def font(locale: str, size: int) -> ImageFont.FreeTypeFont:
    """The bold font with the locale's glyphs (Japanese, Korean and Chinese
    draw some characters differently: each its own face of the collection)."""
    match locale:
        case "ja-JP": return ImageFont.truetype(str(CJK), size, index=0)
        case "ko-KR": return ImageFont.truetype(str(CJK), size, index=1)
        case "zh-CN": return ImageFont.truetype(str(CJK), size, index=2)
        case "ar": return ImageFont.truetype(str(NOTO / "NotoSansArabic-Bold.ttf"), size)
        case _: return ImageFont.truetype(str(NOTO / "NotoSans-Bold.ttf"), size)


def language(locale: str) -> str:
    return {"zh-CN": "zh-Hans", "ar": "ar"}.get(locale, locale[:2])


WIDTH, HEIGHT = 1080, 1920
INK = (0x3D, 0x16, 0x06)


def text_width(draw: ImageDraw.ImageDraw, text: str, f, locale: str) -> float:
    rtl = locale == "ar"
    return draw.textlength(text, font=f, direction="rtl" if rtl else None, language=language(locale))


def lines_for(draw, text: str, f, locale: str, width: int) -> list[str] | None:
    """The headline in at most two lines that fit [width]: as written when
    broken by hand, else one line, else two of about the same length."""
    if "\n" in text:
        lines = text.split("\n")
        return lines if all(text_width(draw, l, f, locale) <= width for l in lines) else None
    if text_width(draw, text, f, locale) <= width:
        return [text]
    words = text.split(" ")
    best = None
    for i in range(1, len(words)):
        pair = [" ".join(words[:i]), " ".join(words[i:])]
        widest = max(text_width(draw, l, f, locale) for l in pair)
        if widest <= width and (best is None or widest < best[0]):
            best = (widest, pair)
    return best[1] if best else None


def headline(img: Image.Image, text: str, locale: str, box: tuple[int, int, int, int]) -> None:
    draw = ImageDraw.Draw(img)
    left, top, right, bottom = box
    size = 84
    while True:
        f = font(locale, size)
        lines = lines_for(draw, text, f, locale, right - left)
        line_height = int(size * 1.28)
        if (lines and len(lines) * line_height <= bottom - top) or size <= 40:
            break
        size -= 2
    lines = lines or [text]
    y = top + (bottom - top - len(lines) * line_height) // 2
    rtl = locale == "ar"
    for line in lines:
        w = text_width(draw, line, f, locale)
        x = left + (right - left - w) / 2
        draw.text(
            (x, y + line_height / 2), line, font=f, fill=INK, anchor="lm",
            direction="rtl" if rtl else None, language=language(locale),
        )
        y += line_height


def background() -> Image.Image:
    """Soft orange, lighter at the top, with a glow behind the phone."""
    stops = [(0.0, (0xFF, 0xF3, 0xEA)), (0.55, (0xF9, 0xD5, 0xBB)), (1.0, (0xF2, 0xB4, 0x8C))]
    column = Image.new("RGB", (1, HEIGHT))
    for y in range(HEIGHT):
        t = y / (HEIGHT - 1)
        for (t0, c0), (t1, c1) in zip(stops, stops[1:]):
            if t0 <= t <= t1:
                k = (t - t0) / (t1 - t0)
                column.putpixel((0, y), tuple(round(a + (b - a) * k) for a, b in zip(c0, c1)))
                break
    img = column.resize((WIDTH, HEIGHT)).convert("RGBA")
    glow = Image.new("L", (WIDTH, HEIGHT), 0)
    ImageDraw.Draw(glow).ellipse((WIDTH / 2 - 560, 1050 - 560, WIDTH / 2 + 560, 1050 + 560), fill=140)
    glow = glow.filter(ImageFilter.GaussianBlur(220))
    img.paste(Image.new("RGBA", (WIDTH, HEIGHT), (255, 255, 255, 255)), (0, 0), glow)
    return img


def compose(shot: Image.Image, title: str, locale: str, base: Image.Image) -> Image.Image:
    img = base.copy()
    # The capture, with rounded corners, a soft shadow and a fine edge.
    shot_w = 740
    shot_h = round(shot_w * shot.height / shot.width)
    x0 = (WIDTH - shot_w) // 2
    y0 = HEIGHT - shot_h - 48
    radius = 64
    shadow = Image.new("RGBA", img.size, (0, 0, 0, 0))
    ImageDraw.Draw(shadow).rounded_rectangle(
        (x0, y0 + 18, x0 + shot_w, y0 + shot_h + 18), radius, fill=(0x6B, 0x2A, 0x08, 90))
    img = Image.alpha_composite(img, shadow.filter(ImageFilter.GaussianBlur(28)))
    mask = Image.new("L", (shot_w, shot_h), 0)
    ImageDraw.Draw(mask).rounded_rectangle((0, 0, shot_w - 1, shot_h - 1), radius, fill=255)
    img.paste(shot.convert("RGB").resize((shot_w, shot_h), Image.LANCZOS), (x0, y0), mask)
    edge = Image.new("RGBA", img.size, (0, 0, 0, 0))
    ImageDraw.Draw(edge).rounded_rectangle(
        (x0, y0, x0 + shot_w, y0 + shot_h), radius, outline=(0x3D, 0x16, 0x06, 40), width=2)
    img = Image.alpha_composite(img, edge)
    # The headline, between the top and the capture.
    headline(img, title, locale, (60, 40, WIDTH - 60, y0 - 24))
    return img.convert("RGB")


def sheet(images: list[Image.Image]) -> Image.Image:
    w, h, gap = 270, 480, 12
    out = Image.new("RGB", (len(images) * (w + gap) + gap, h + 2 * gap), (0x77, 0x77, 0x77))
    for i, image in enumerate(images):
        out.paste(image.resize((w, h), Image.LANCZOS), (gap + i * (w + gap), gap))
    return out


def main() -> None:
    root = Path(sys.argv[1] if len(sys.argv) > 1 else "store/android")
    sheets = Path(sys.argv[2]) if len(sys.argv) > 2 else None
    base = background()
    for locale, titles in sorted(HEADLINES.items()):
        made = []
        for scene in SCENES:
            raw = root / locale / "raw" / f"{scene}.png"
            if not raw.exists():
                continue
            image = compose(Image.open(raw), titles[scene.split("_", 1)[1]], locale, base)
            image.save(root / locale / f"{scene}.png", optimize=True)
            made.append(image)
        if sheets and made:
            sheets.mkdir(parents=True, exist_ok=True)
            sheet(made).save(sheets / f"{locale}.png")
        print(f"{locale}: {len(made)} images")


if __name__ == "__main__":
    main()
