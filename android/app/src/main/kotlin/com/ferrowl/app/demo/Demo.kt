package com.ferrowl.app.demo

import com.ferrowl.app.BuildConfig
import com.ferrowl.core.Account
import com.ferrowl.core.Attachment
import com.ferrowl.core.CalendarEvent
import com.ferrowl.core.Folder
import com.ferrowl.core.FolderKind
import com.ferrowl.core.Header
import com.ferrowl.core.Invitation
import com.ferrowl.core.Message
import com.ferrowl.core.PgpInfo
import com.ferrowl.core.Protocol
import com.ferrowl.core.Signature
import com.ferrowl.core.translate
import java.time.LocalDate
import java.time.ZoneId
import java.util.Locale

/**
 * The `demo` build type only (a debug build installed as
 * `com.ferrowl.app.demo`, beside the real app): made-up mail for the Google
 * Play screenshots, in the app's language. Started from the command line:
 *
 *     adb shell am start -S -n com.ferrowl.app.demo/com.ferrowl.app.MainActivity --es ferrowl_demo inbox
 *
 * Scenes: `inbox`, `message` (a newsletter with its pictures blocked),
 * `undo`, `accounts` (the drawer), `pgp` (a signed and encrypted message),
 * `calendar`, `invite` (a message with an invitation).
 *
 * Nothing real is read or written: the accounts, folders, messages and
 * events below live in memory only; the engine works in an empty folder of
 * its own and is never connected ([DemoEngine]); every server action is
 * refused quietly, and the saved accounts and settings, push server,
 * Google Play Billing, widget and background checks are left alone.
 * [on] is a constant false in every other build, so R8 removes all of
 * this from release builds.
 */
object Demo {
    const val on: Boolean = BuildConfig.DEMO

    /** The intent extra with the scene's name. */
    const val EXTRA = "ferrowl_demo"

    enum class Scene { INBOX, MESSAGE, UNDO, ACCOUNTS, PGP, CALENDAR, INVITE }

    /** The scene asked for when the app was started (see [EXTRA]). */
    @Volatile var scene: Scene = Scene.INBOX
        private set

    /** The scene was set up once (not again when the screen is rebuilt). */
    @Volatile var staged = false

    fun start(name: String?) {
        scene = Scene.entries.firstOrNull { it.name.equals(name, ignoreCase = true) } ?: Scene.INBOX
        staged = false
    }

    /** A sample text in the app's language. */
    fun text(key: String): String =
        DemoTexts.of(Locale.getDefault().language)?.get(key) ?: DemoTexts.of("en")?.get(key) ?: key

    // ------------------------------------------------------------ accounts

    val accounts: List<Account> = listOf(
        account("Alex Morgan", "alex.morgan@example.com"),
        account("Alex Morgan", "alex@example.org"),
        account("Alex M.", "alex.m@example.net"),
    )

    private fun account(name: String, email: String) = Account(
        displayName = name,
        email = email,
        username = email,
        password = "",
        incomingHost = "imap.example.com",
        incomingPort = 993u,
        smtpHost = "smtp.example.com",
        smtpPort = 465u,
        protocol = Protocol.IMAP,
        pop3KeepDays = null,
        pgpPassphrase = "",
    )

    /** Unread mail in the inbox of each account (the drawer). */
    fun accountUnread(): Map<String, Int> = mapOf(
        accounts[0].email to inbox().count { !it.seen },
        accounts[1].email to 7,
        accounts[2].email to 2,
    )

    // ------------------------------------------------------------- folders

    fun folders(): List<Folder> {
        fun folder(id: String, name: String, kind: FolderKind) = Folder(id, name, listOf(name), kind, true)
        return listOf(
            folder("INBOX", translate("folder_inbox"), FolderKind.INBOX),
            folder("Drafts", translate("folder_drafts"), FolderKind.DRAFTS),
            folder("Sent", translate("folder_sent"), FolderKind.SENT),
            folder("Archive", translate("folder_archive"), FolderKind.ARCHIVE),
            folder("Junk", translate("folder_spam"), FolderKind.SPAM),
            folder("Trash", translate("folder_trash"), FolderKind.TRASH),
            folder("Projects", text("f_projects"), FolderKind.OTHER),
            folder("Travel", text("f_travel"), FolderKind.OTHER),
        )
    }

    fun unread(): Map<String, UInt> =
        mapOf("INBOX" to inbox().count { !it.seen }.toUInt(), "Junk" to 3u, "Projects" to 2u)

    // ------------------------------------------------------------ messages

    const val NEWSLETTER = 113u
    const val SIGNED = 112u
    const val INVITATION = 111u

    /** The message the `undo` scene moves to the Trash. */
    const val TRASHED = 109u

    private val now: Long = System.currentTimeMillis() / 1000

    fun headers(folderId: String): List<Header> = if (folderId == "INBOX") inbox() else emptyList()

    fun inbox(): List<Header> {
        fun header(
            uid: UInt,
            from: String,
            address: String,
            subject: String,
            preview: String,
            minutesAgo: Long,
            seen: Boolean,
            attachment: Boolean = false,
            flagged: Boolean = false,
            id: String = "",
            parents: List<String> = emptyList(),
        ) = Header(
            uid = uid,
            from = from,
            subject = subject,
            preview = preview,
            timestamp = now - minutesAgo * 60,
            seen = seen,
            hasAttachment = attachment,
            messageId = id.ifEmpty { "demo-$uid@example.com" },
            parents = parents,
            flagged = flagged,
            size = 24_000u,
            fromAddress = address,
        )
        val hike = text("hike_subject")
        return listOf(
            header(
                114u, "Sofia Marino", "sofia.marino@example.com", "Re: $hike", text("hike_3"), 12, seen = false,
                id = "hike-3@example.com", parents = listOf("hike-2@example.com", "hike-1@example.com"),
            ),
            header(NEWSLETTER, text("news_from"), "hello@news.example.org", text("news_subject"), text("news_preview"), 38, seen = false),
            header(
                SIGNED, "Daniel Weber", "daniel.weber@example.org", text("pgp_subject"), text("pgp_preview"), 95,
                seen = true, attachment = true, flagged = true,
            ),
            header(INVITATION, "Yuki Tanaka", "yuki.tanaka@example.org", text("inv_subject"), text("inv_preview"), 170, seen = false),
            header(
                110u, "Marco Rossi", "marco.rossi@example.net", "Re: $hike", text("hike_2"), 200, seen = true,
                id = "hike-2@example.com", parents = listOf("hike-1@example.com"),
            ),
            header(TRASHED, text("lib_from"), "desk@library.example.org", text("lib_subject"), text("lib_preview"), 300, seen = true),
            header(108u, "Sofia Marino", "sofia.marino@example.com", hike, text("hike_1"), 420, seen = true, id = "hike-1@example.com"),
            header(
                107u, "Lucas Ferreira", "lucas.ferreira@example.com", text("photos_subject"), text("photos_preview"), 26 * 60,
                seen = true, attachment = true, flagged = true,
            ),
            header(106u, "Amira Haddad", "amira.haddad@example.org", text("budget_subject"), text("budget_preview"), 30 * 60, seen = false),
            header(105u, "Clara Dubois", "clara.dubois@example.net", text("dinner_subject"), text("dinner_preview"), 50 * 60, seen = true),
            header(
                104u, "Min-jun Park", "minjun.park@example.org", text("slides_subject"), text("slides_preview"), 73 * 60,
                seen = true, attachment = true,
            ),
            header(103u, text("coop_from"), "orders@market.example.com", text("coop_subject"), text("coop_preview"), 98 * 60, seen = true),
        )
    }

    fun search(folderId: String, query: String): List<Header> = headers(folderId).filter {
        it.subject.contains(query, ignoreCase = true) || it.from.contains(query, ignoreCase = true) ||
            it.preview.contains(query, ignoreCase = true)
    }

    /** A whole message of the inbox, or null for one that is not there. */
    fun message(uid: UInt): Message? {
        val h = inbox().firstOrNull { it.uid == uid } ?: return null
        val message = Message(
            uid = uid,
            from = h.from,
            fromAddress = h.fromAddress,
            to = "Alex Morgan",
            cc = "",
            toAddresses = listOf(accounts[0].email),
            ccAddresses = emptyList(),
            replyToAddresses = emptyList(),
            subject = h.subject,
            timestamp = h.timestamp,
            messageId = h.messageId,
            references = h.parents,
            text = h.preview,
            html = null,
            inline = emptyMap(),
            attachments = emptyList(),
            dkimPass = true,
            pgp = null,
            calendar = null,
            invitation = null,
        )
        return when (uid) {
            NEWSLETTER -> message.copy(
                to = accounts[0].email,
                html = newsletterHtml(),
                text = text("news_title") + "\n\n" + text("news_intro"),
            )
            SIGNED -> message.copy(
                text = text("pgp_body"),
                pgp = PgpInfo(encrypted = true, signature = Signature.VALID, error = null),
                attachments = listOf(Attachment("contract-v3.pdf", ByteArray(248_312))),
            )
            INVITATION -> {
                val start = day(2, 10)
                message.copy(
                    text = text("inv_body"),
                    invitation = Invitation(
                        method = "REQUEST",
                        uid = "design-review@example.org",
                        summary = text("inv_summary"),
                        location = text("inv_location"),
                        start = start,
                        end = start + 3600,
                        allDay = false,
                        firstDay = null,
                        lastDay = null,
                        recurring = false,
                        organizer = h.fromAddress,
                        organizerName = h.from,
                        answers = emptyList(),
                    ),
                )
            }
            else -> message
        }
    }

    /** A newsletter with remote pictures (blocked, so the bar to load them shows). */
    private fun newsletterHtml(): String {
        val dir = if (Locale.getDefault().language == "ar") "rtl" else "ltr"
        fun item(n: Int) =
            """<tr><td class="num">$n</td><td class="it"><b>${text("news_${n}_title")}</b><br>${text("news_$n")}</td></tr>"""
        return """
            <!doctype html><html dir="$dir"><head><style>
            body { background: #f4efe8; }
            .wrap { max-width: 600px; margin: 0 auto; background: #fffdf9; color: #2b2622; font-family: sans-serif; }
            .mast { background: #2f4a3c; color: #f5efe6; padding: 20px 22px; }
            .brand { font-size: 22px; font-weight: 800; letter-spacing: 3px; }
            .issue { font-size: 13px; opacity: .75; margin-top: 4px; }
            .hero { height: 150px; background: linear-gradient(135deg, #eadfcf, #d6c2a6); }
            .body { padding: 20px 22px 8px; }
            h1 { font-size: 24px; line-height: 1.25; margin: 0 0 10px; color: #2f4a3c; }
            p { font-size: 16px; line-height: 1.5; margin: 0 0 14px; }
            table { border-collapse: collapse; width: 100%; }
            td { padding: 10px 0; vertical-align: top; font-size: 15px; line-height: 1.45; border-top: 1px solid #eee4d8; }
            .num { width: 40px; font-size: 22px; font-weight: 800; color: #c0622b; }
            .btn { display: inline-block; margin: 18px 0 22px; padding: 12px 22px; border-radius: 24px; background: #c0622b;
                   color: #fff; text-decoration: none; font-weight: 700; }
            .foot { padding: 16px 22px 24px; font-size: 12px; color: #8a8178; background: #f4efe8; }
            </style></head><body><div class="wrap">
            <div class="mast"><div class="brand">FIELD NOTES</div><div class="issue">№ 48</div></div>
            <div class="hero"><img src="https://news.example.org/img/48/hero.jpg" alt=""></div>
            <div class="body"><h1>${text("news_title")}</h1><p>${text("news_intro")}</p>
            <table>${item(1)}${item(2)}${item(3)}</table>
            <a class="btn" href="https://news.example.org/48">${text("news_button")}</a></div>
            <div class="foot">${text("news_footer")}</div>
            <img src="https://news.example.org/open/48.gif" width="1" height="1" alt="">
            </div></body></html>
        """.trimIndent()
    }

    // ------------------------------------------------------------ calendar

    /** [days] from today at [hour] (local time), in seconds. */
    private fun day(days: Long, hour: Int, minute: Int = 0): Long =
        LocalDate.now().plusDays(days).atTime(hour, minute).atZone(ZoneId.systemDefault()).toEpochSecond()

    fun agenda(from: Long, to: Long): List<CalendarEvent> {
        fun event(id: String, summary: String, location: String, start: Long, minutes: Long, color: String, recurring: Boolean = false) =
            CalendarEvent(
                uid = id,
                summary = summary,
                location = location,
                description = "",
                start = start,
                end = start + minutes * 60,
                allDay = false,
                firstDay = null,
                lastDay = null,
                recurring = recurring,
                organizer = "",
                calendar = "",
                color = color,
                url = "demo:$id",
                calendarUrl = "demo:",
            )
        val work = "#1E5AA8"
        val personal = "#A63A0C"
        val sport = "#2F6B45"
        val birthday = LocalDate.now().plusDays(4).toString()
        val events = listOf(
            event("standup", text("ev_standup"), text("ev_call"), day(0, 9, 30), 15, work, recurring = true),
            event("lunch", text("ev_lunch"), text("ev_lunch_loc"), day(0, 13), 60, personal),
            event("yoga", text("ev_yoga"), "", day(1, 18, 30), 60, sport, recurring = true),
            event("review", text("inv_summary"), text("inv_location"), day(2, 10), 60, work),
            event("standup2", text("ev_standup"), text("ev_call"), day(2, 9, 30), 15, work, recurring = true),
            event("dentist", text("ev_dentist"), "", day(3, 8, 30), 30, personal),
            event("birthday", text("ev_birthday"), "", day(4, 0), 24 * 60, personal, recurring = true)
                .copy(allDay = true, firstDay = birthday, lastDay = birthday),
            event("hike", text("ev_hike"), text("ev_hike_loc"), day(5, 8), 360, sport),
        )
        return events.filter { it.start < to && it.end > from }.sortedBy { it.start }
    }
}
