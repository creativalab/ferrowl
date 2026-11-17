// Makes the App Store screenshots (6.9" iPhone, 1320×2868) from the raw
// phone captures of store/tools/capture_ios.sh: a headline in the store's
// language over a soft orange background, the capture below it.
//
//   swift store/tools/compose_ios.swift [store/ios] [contact-sheet-dir]
//
// Reads <locale>/screenshots/raw/NN_scene.png, writes
// <locale>/screenshots/NN_scene.png. With a second argument, one small
// sheet per locale with all its images goes there (to check them at a glance).
import CoreGraphics
import CoreText
import Foundation
import ImageIO
import UniformTypeIdentifiers

let scenes = ["01_inbox", "02_message", "03_undo", "04_accounts", "05_pgp", "06_calendar"]

/// Headlines per store locale, in the order of `scenes`.
let headlines: [String: [String]] = [
    "en-US": ["Email that doesn't track you", "Senders never know when you read", "Undo with one tap",
              "All your mailboxes in one app", "OpenPGP signing and encryption", "Calendar and invitations"],
    "it": ["La posta che non ti traccia", "Chi scrive non sa quando leggi", "Annulla con un tocco",
           "Tutte le tue caselle in un'app", "Firma e cifratura OpenPGP", "Calendario e inviti"],
    "es-ES": ["Correo que no te rastrea", "Nadie sabe cuándo lees", "Deshaz con un toque",
              "Todos tus buzones en una app", "Firma y cifrado OpenPGP", "Calendario e invitaciones"],
    "fr-FR": ["Des e-mails qui ne vous pistent pas", "Personne ne sait quand vous lisez", "Annulez d'un geste",
              "Toutes vos boîtes dans une app", "Signature et chiffrement OpenPGP", "Agenda et invitations"],
    "de-DE": ["E-Mail, die dich nicht verfolgt", "Absender sehen nicht, wann du liest", "Mit einem Tipp rückgängig",
              "Alle Postfächer in einer App", "Signieren und verschlüsseln mit OpenPGP", "Kalender und Einladungen"],
    "pt-BR": ["E-mail que não rastreia você", "Ninguém sabe quando você lê", "Desfaça com um toque",
              "Todas as suas caixas em um app", "Assinatura e criptografia OpenPGP", "Agenda e convites"],
    "ja": ["あなたを追跡しない\nメール", "いつ読んだか、\n送信者にはわかりません", "ワンタップで取り消し",
           "すべてのメールを\n1つのアプリで", "OpenPGPで\n署名と暗号化", "カレンダーと招待"],
    "ko": ["나를 추적하지 않는 이메일", "읽은 시점을\n보낸 사람은 알 수 없어요", "한 번의 탭으로 실행 취소",
           "모든 메일함을\n하나의 앱에서", "OpenPGP 서명과 암호화", "캘린더와 초대"],
    "zh-Hans": ["不追踪你的邮件", "发件人无从知道\n你何时阅读", "轻点一下即可撤销",
                "所有邮箱，尽在一个应用", "OpenPGP 签名与加密", "日历与邀请"],
    "ar-SA": ["بريد لا يتتبّعك", "لا يعرف المرسِل متى تقرأ", "تراجع بلمسة واحدة",
              "كل صناديق بريدك في تطبيق واحد", "توقيع وتشفير OpenPGP", "التقويم والدعوات"],
]

/// The language of a store locale, for the font's glyphs (Japanese and
/// Chinese draw some characters differently).
func language(_ locale: String) -> String {
    switch locale {
    case "zh-Hans": "zh-Hans"
    case "ar-SA": "ar"
    default: String(locale.prefix(2))
    }
}

let width = 1320, height = 2868
let space = CGColorSpace(name: CGColorSpace.sRGB)!

func color(_ hex: UInt32, _ alpha: CGFloat = 1) -> CGColor {
    CGColor(srgbRed: CGFloat((hex >> 16) & 0xFF) / 255, green: CGFloat((hex >> 8) & 0xFF) / 255,
            blue: CGFloat(hex & 0xFF) / 255, alpha: alpha)
}

func load(_ url: URL) -> CGImage? {
    guard let source = CGImageSourceCreateWithURL(url as CFURL, nil) else { return nil }
    return CGImageSourceCreateImageAtIndex(source, 0, nil)
}

func save(_ image: CGImage, _ url: URL) {
    let dest = CGImageDestinationCreateWithURL(url as CFURL, UTType.png.identifier as CFString, 1, nil)!
    CGImageDestinationAddImage(dest, image, nil)
    CGImageDestinationFinalize(dest)
}

/// The headline set in the system's bold font, as large as fits in two lines.
func headline(_ text: String, language: String, rtl: Bool, in box: CGRect) -> CTFrame {
    var size: CGFloat = 108
    while true {
        let font = CTFontCreateUIFontForLanguage(.system, size, language as CFString)!
        let bold = CTFontCreateCopyWithSymbolicTraits(font, size, nil, .traitBold, .traitBold) ?? font
        var alignment = CTTextAlignment.center
        var direction = rtl ? CTWritingDirection.rightToLeft : .leftToRight
        var spacing: CGFloat = size * 0.08
        let style = withUnsafeBytes(of: &alignment) { a in
            withUnsafeBytes(of: &direction) { d in
                withUnsafeBytes(of: &spacing) { s in
                    let settings = [
                        CTParagraphStyleSetting(spec: .alignment, valueSize: a.count, value: a.baseAddress!),
                        CTParagraphStyleSetting(spec: .baseWritingDirection, valueSize: d.count, value: d.baseAddress!),
                        CTParagraphStyleSetting(spec: .lineSpacingAdjustment, valueSize: s.count, value: s.baseAddress!),
                    ]
                    return CTParagraphStyleCreate(settings, settings.count)
                }
            }
        }
        let attributes: [NSAttributedString.Key: Any] = [
            NSAttributedString.Key(kCTFontAttributeName as String): bold,
            NSAttributedString.Key(kCTForegroundColorAttributeName as String): color(0x3D1606),
            NSAttributedString.Key(kCTParagraphStyleAttributeName as String): style,
            NSAttributedString.Key(kCTLanguageAttributeName as String): language,
        ]
        let string = NSAttributedString(string: text, attributes: attributes)
        let setter = CTFramesetterCreateWithAttributedString(string)
        let fit = CTFramesetterSuggestFrameSizeWithConstraints(setter, CFRange(), nil,
                                                               CGSize(width: box.width, height: .greatestFiniteMagnitude), nil)
        let frame = CTFramesetterCreateFrame(setter, CFRange(), CGPath(rect: CGRect(x: 0, y: 0, width: box.width, height: 4000),
                                                                      transform: nil), nil)
        let lines = CFArrayGetCount(CTFrameGetLines(frame))
        if (fit.height <= box.height && lines <= 2) || size < 50 {
            // Two lines of about the same length: the narrowest width that
            // still takes two lines (no lone word on the second).
            func lineCount(_ w: CGFloat) -> Int {
                let f = CTFramesetterCreateFrame(setter, CFRange(), CGPath(rect: CGRect(x: 0, y: 0, width: w, height: 4000),
                                                                          transform: nil), nil)
                return CFArrayGetCount(CTFrameGetLines(f))
            }
            // Lines broken by hand (Chinese, Japanese, Korean) stay as they are.
            var w = box.width
            while lines == 2, !text.contains("\n"), w > box.width * 0.5, lineCount(w - 10) == 2 { w -= 10 }
            let used = CTFramesetterSuggestFrameSizeWithConstraints(setter, CFRange(), nil,
                                                                    CGSize(width: w, height: .greatestFiniteMagnitude), nil)
            // Centered in the box.
            let rect = CGRect(x: box.minX + (box.width - w) / 2, y: box.minY + (box.height - used.height) / 2, width: w,
                              height: ceil(used.height) + 8)
            return CTFramesetterCreateFrame(setter, CFRange(), CGPath(rect: rect, transform: nil), nil)
        }
        size -= 4
    }
}

func compose(_ shot: CGImage, title: String, language: String, rtl: Bool) -> CGImage {
    let ctx = CGContext(data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: 0, space: space,
                        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
    // Coordinates from the top, as on screen.
    ctx.translateBy(x: 0, y: CGFloat(height))
    ctx.scaleBy(x: 1, y: -1)

    // Soft orange, lighter at the top, with a glow behind the phone.
    let gradient = CGGradient(colorsSpace: space, colors: [color(0xFFF3EA), color(0xF9D5BB), color(0xF2B48C)] as CFArray,
                              locations: [0, 0.55, 1])!
    ctx.drawLinearGradient(gradient, start: .zero, end: CGPoint(x: 0, y: height), options: [])
    let glow = CGGradient(colorsSpace: space, colors: [color(0xFFFFFF, 0.55), color(0xFFFFFF, 0)] as CFArray,
                          locations: [0, 1])!
    ctx.drawRadialGradient(glow, startCenter: CGPoint(x: 660, y: 1500), startRadius: 0,
                           endCenter: CGPoint(x: 660, y: 1500), endRadius: 900, options: [])

    // The capture, with rounded corners and a soft shadow.
    let shotWidth: CGFloat = 1060
    let shotHeight = shotWidth * CGFloat(shot.height) / CGFloat(shot.width)
    let frame = CGRect(x: (CGFloat(width) - shotWidth) / 2, y: CGFloat(height) - shotHeight - 96,
                       width: shotWidth, height: shotHeight)
    let radius: CGFloat = 112
    let path = CGPath(roundedRect: frame, cornerWidth: radius, cornerHeight: radius, transform: nil)
    ctx.saveGState()
    ctx.setShadow(offset: CGSize(width: 0, height: 24), blur: 70, color: color(0x6B2A08, 0.35))
    ctx.addPath(path)
    ctx.setFillColor(color(0xFFFFFF))
    ctx.fillPath()
    ctx.restoreGState()
    ctx.saveGState()
    ctx.addPath(path)
    ctx.clip()
    // Images draw upside down in the flipped context: flipped back locally.
    ctx.translateBy(x: frame.minX, y: frame.maxY)
    ctx.scaleBy(x: 1, y: -1)
    ctx.interpolationQuality = .high
    ctx.draw(shot, in: CGRect(x: 0, y: 0, width: frame.width, height: frame.height))
    ctx.restoreGState()
    ctx.addPath(path)
    ctx.setStrokeColor(color(0x3D1606, 0.12))
    ctx.setLineWidth(3)
    ctx.strokePath()

    // The headline, between the top and the capture (Core Text draws
    // bottom-up: back to the unflipped space).
    ctx.saveGState()
    ctx.scaleBy(x: 1, y: -1)
    ctx.translateBy(x: 0, y: -CGFloat(height))
    let boxTop: CGFloat = 150, boxBottom = frame.minY - 60
    let box = CGRect(x: 80, y: CGFloat(height) - boxBottom, width: CGFloat(width) - 160, height: boxBottom - boxTop)
    CTFrameDraw(headline(title, language: language, rtl: rtl, in: box), ctx)
    ctx.restoreGState()
    return ctx.makeImage()!
}

/// All images of a locale side by side, small.
func sheet(_ images: [CGImage]) -> CGImage {
    let w = 330, h = 717, gap = 12
    let ctx = CGContext(data: nil, width: images.count * (w + gap) + gap, height: h + 2 * gap, bitsPerComponent: 8,
                        bytesPerRow: 0, space: space, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
    ctx.setFillColor(color(0x777777))
    ctx.fill(CGRect(x: 0, y: 0, width: ctx.width, height: ctx.height))
    ctx.interpolationQuality = .high
    for (i, image) in images.enumerated() {
        ctx.draw(image, in: CGRect(x: gap + i * (w + gap), y: gap, width: w, height: h))
    }
    return ctx.makeImage()!
}

let args = CommandLine.arguments
let root = URL(fileURLWithPath: args.count > 1 ? args[1] : "store/ios")
let sheets = args.count > 2 ? URL(fileURLWithPath: args[2]) : nil
for (locale, titles) in headlines.sorted(by: { $0.key < $1.key }) {
    let dir = root.appendingPathComponent("\(locale)/screenshots")
    var made: [CGImage] = []
    for (i, scene) in scenes.enumerated() {
        guard let shot = load(dir.appendingPathComponent("raw/\(scene).png")) else {
            print("\(locale) \(scene): no capture")
            continue
        }
        let image = compose(shot, title: titles[i], language: language(locale), rtl: locale == "ar-SA")
        save(image, dir.appendingPathComponent("\(scene).png"))
        made.append(image)
    }
    if let sheets, !made.isEmpty {
        try? FileManager.default.createDirectory(at: sheets, withIntermediateDirectories: true)
        save(sheet(made), sheets.appendingPathComponent("\(locale).png"))
    }
    print("\(locale): \(made.count) images")
}
