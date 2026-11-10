package com.ferrowl.app.bridge

import android.Manifest
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import com.ferrowl.app.R
import com.ferrowl.app.data.Billing
import com.ferrowl.app.data.MailRepository
import com.ferrowl.core.Account
import com.ferrowl.core.Capability
import com.ferrowl.core.Engine
import com.ferrowl.core.FerrowlException
import com.ferrowl.core.FerrowlLink
import com.ferrowl.core.Header
import com.ferrowl.core.InboxPeek
import com.ferrowl.core.MailInsight
import com.ferrowl.core.Outgoing
import com.ferrowl.core.capabilityNeedsConfirmation
import com.ferrowl.core.capabilityNeedsPro
import com.ferrowl.core.mailInsights
import com.ferrowl.core.markdownToHtml
import com.ferrowl.core.messageLink
import com.ferrowl.core.parseLink
import com.ferrowl.core.translate
import dagger.hilt.android.qualifiers.ApplicationContext
import java.util.Locale
import java.util.UUID
import javax.inject.Inject
import javax.inject.Singleton
import kotlinx.coroutines.flow.first
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put

/**
 * Ferrowl Bridge on Android: what automation apps (Tasker, MacroDroid,
 * Automate…) may ask of Ferrowl through intents, and the events they get.
 *
 * Requests are broadcasts to Ferrowl ([BridgeReceiver]) with the app's key
 * in the `token` extra and an optional `request` id echoed in the answer:
 * - [ACTION_UNREAD] (`account`, `limit`): unread mail from the server;
 * - [ACTION_SEARCH] (`query`, `account`, `limit`): mail stored on the phone;
 * - [ACTION_AGENDA] (`day` YYYY-MM-DD, `days`, `account`): synced events;
 * - [ACTION_CONTACTS] (`query`, `account`, `limit`): the address book;
 * - [ACTION_DETAILS] (`link`): a message's text, attachments, shipment,
 *   code and bill, without marking it read;
 * - [ACTION_ARCHIVE], [ACTION_MARK_READ], [ACTION_MARK_UNREAD] (`link`: the
 *   messages' `ferrowl://open` links from `json`): Ferrowl Pro, confirmed;
 * - [ACTION_SEND] (`to`, `subject`, `body`, `account`): Ferrowl Pro; the
 *   user confirms each message in a notification unless they turned that off.
 *
 * Answers ([EVENT_RESULT]) and new-mail events ([EVENT_NEW_MAIL]) are sent
 * only to the connected app's package, with `ok`, `error`, `count`, `text`
 * (a summary to read out) and `json` (the messages).
 */
@Singleton
class Bridge @Inject constructor(
    @ApplicationContext private val context: Context,
    private val connections: Connections,
    private val repo: MailRepository,
    private val billing: Billing,
) {
    suspend fun handle(intent: Intent) {
        // An unknown key gets no answer: there is nobody to send it to.
        val connection = connections.find(intent.getStringExtra(EXTRA_TOKEN)) ?: return
        val action = intent.action ?: return
        val request = intent.getStringExtra(EXTRA_REQUEST).orEmpty()
        val name = action.substringAfterLast('.')
        val result = try {
            when (action) {
                ACTION_UNREAD -> requireRead(connection) { unread(intent) }
                ACTION_SEARCH -> requireRead(connection) { search(intent) }
                ACTION_AGENDA -> requireRead(connection) { agenda(intent) }
                ACTION_CONTACTS -> requireRead(connection) { contacts(intent) }
                ACTION_DETAILS -> requireRead(connection) { details(intent) }
                ACTION_SEND -> send(connection, intent, request)
                ACTION_ARCHIVE, ACTION_MARK_READ, ACTION_MARK_UNREAD -> change(connection, intent, request)
                else -> Result(ok = false, error = "unknown action")
            }
        } catch (e: FerrowlException) {
            Result(ok = false, error = e.message.orEmpty())
        } catch (e: BridgeError) {
            Result(ok = false, error = e.message.orEmpty())
        }
        connections.used(connection, name, if (result.ok) (result.status ?: "ok") else result.error)
        reply(connection, action, request, result)
    }

    private class BridgeError(message: String) : Exception(message)

    /** What an answer carries. */
    private data class Result(
        val ok: Boolean,
        val error: String = "",
        val status: String? = null,
        val count: Int = 0,
        val text: String = "",
        val messages: List<Pair<String, Header>> = emptyList(),
        /** Agenda or contacts, already as JSON (instead of [messages]). */
        val json: String? = null,
    )

    private suspend fun requireRead(connection: Connection, run: suspend () -> Result): Result =
        if (connection.read) run() else Result(ok = false, error = translate("integrations_denied"))

    /** The accounts asked for, among those Ferrowl may use (the first one without Pro). */
    private suspend fun accounts(wanted: String?): List<Account> {
        val all = repo.accountList.first()
        val usable = if (billing.pro.value) all else all.take(1)
        val chosen = if (wanted.isNullOrBlank()) usable else usable.filter { it.email.equals(wanted.trim(), ignoreCase = true) }
        if (chosen.isEmpty()) throw BridgeError(translate("intent_no_account"))
        return chosen
    }

    private fun limit(intent: Intent): Int =
        (intent.getStringExtra(EXTRA_LIMIT)?.toIntOrNull() ?: intent.getIntExtra(EXTRA_LIMIT, DEFAULT_LIMIT))
            .coerceIn(1, MAX_LIMIT)

    private suspend fun unread(intent: Intent): Result {
        val limit = limit(intent)
        var total = 0
        val found = mutableListOf<Pair<String, Header>>()
        for (account in accounts(intent.getStringExtra(EXTRA_ACCOUNT))) {
            val peek = repo.peekInbox(account.email, 0, limit) ?: continue
            total += peek.freshCount.toInt()
            found += peek.fresh.map { account.email to it }
        }
        val sorted = found.sortedByDescending { it.second.timestamp }.take(limit)
        val text = if (total == 0) {
            translate("intent_no_unread")
        } else {
            summary(translate("intent_unread_count").replace("{n}", total.toString()), sorted)
        }
        return Result(ok = true, count = total, text = text, messages = sorted)
    }

    private suspend fun search(intent: Intent): Result {
        val query = intent.getStringExtra(EXTRA_QUERY).orEmpty()
        if (query.isBlank()) throw BridgeError(translate("intent_ask_query"))
        val limit = limit(intent)
        val found = accounts(intent.getStringExtra(EXTRA_ACCOUNT)).flatMap { a ->
            repo.searchAccount(a, query, limit).map { a.email to it }
        }.sortedByDescending { it.second.timestamp }.take(limit)
        val text = if (found.isEmpty()) {
            translate("intent_none_found")
        } else {
            summary(translate("intent_found").replace("{n}", found.size.toString()), found)
        }
        return Result(ok = true, count = found.size, text = text, messages = found)
    }

    /** Events of `day` (YYYY-MM-DD, today if missing) and the `days` after it. */
    private suspend fun agenda(intent: Intent): Result {
        val zone = java.time.ZoneId.systemDefault()
        val first = intent.getStringExtra(EXTRA_DAY)?.let { runCatching { java.time.LocalDate.parse(it) }.getOrNull() }
            ?: java.time.LocalDate.now(zone)
        val days = (intent.getStringExtra(EXTRA_DAYS)?.toIntOrNull() ?: intent.getIntExtra(EXTRA_DAYS, 1)).coerceIn(1, 31)
        val from = first.atStartOfDay(zone).toEpochSecond()
        val to = first.plusDays(days.toLong()).atStartOfDay(zone).toEpochSecond()
        val events = accounts(intent.getStringExtra(EXTRA_ACCOUNT))
            .flatMap { repo.agendaFor(it, from, to) }
            .sortedBy { it.start }
        val time = java.time.format.DateTimeFormatter.ofPattern("HH:mm")
        val lines = events.take(5).map { e ->
            if (e.allDay) e.summary else "${java.time.Instant.ofEpochSecond(e.start).atZone(zone).format(time)} ${e.summary}"
        }
        val text = if (events.isEmpty()) {
            translate("intent_no_events")
        } else {
            (listOf(translate("intent_events_count").replace("{n}", events.size.toString())) + lines).joinToString("\n")
        }
        val json = JsonArray(
            events.map { e ->
                buildJsonObject {
                    put("title", e.summary)
                    put("start", e.start)
                    put("end", e.end)
                    put("allDay", e.allDay)
                    put("location", e.location)
                    put("calendar", e.calendar)
                    put("organizer", e.organizer)
                }
            },
        ).toString()
        return Result(ok = true, count = events.size, text = text, json = json)
    }

    /**
     * A message's text, attachments and what Ferrowl recognizes in it
     * (shipment, code, bill), by its `ferrowl://open` link; not marked read.
     */
    private suspend fun details(intent: Intent): Result {
        val open = intent.getStringExtra(EXTRA_LINK)?.lines()?.firstNotNullOfOrNull { parseLink(it.trim()) as? FerrowlLink.Open }
            ?: throw BridgeError("link?")
        val account = accounts(open.account).first()
        val m = repo.peekMessage(account, open.folder, open.uid)
        val found = mailInsights(m)
        val json = buildJsonObject {
            put("account", account.email)
            put("from", m.from)
            put("address", m.fromAddress)
            put("subject", m.subject)
            put("date", m.timestamp)
            put("text", m.text.take(MAX_TEXT))
            put("attachments", JsonArray(m.attachments.map { JsonPrimitive(it.name) }))
            for (i in found) {
                when (i) {
                    is MailInsight.Shipment -> put(
                        "shipment",
                        buildJsonObject {
                            put("carrier", i.carrier)
                            put("number", i.number)
                            put("url", i.url)
                        },
                    )
                    is MailInsight.Code -> put("code", i.code)
                    is MailInsight.Payment -> put(
                        "payment",
                        buildJsonObject {
                            put("number", i.number)
                            put("amount", i.amount)
                            put("due", i.due)
                        },
                    )
                }
            }
        }.toString()
        val summary = listOfNotNull(
            m.subject,
            found.filterIsInstance<MailInsight.Shipment>().firstOrNull()?.let { "${it.carrier} ${it.number}".trim() },
            found.filterIsInstance<MailInsight.Code>().firstOrNull()?.code,
            found.filterIsInstance<MailInsight.Payment>().firstOrNull()?.let { "${it.amount} ${it.due}".trim() },
        ).filter { it.isNotBlank() }.joinToString("\n")
        return Result(ok = true, count = 1, text = summary, json = json)
    }

    private suspend fun contacts(intent: Intent): Result {
        val query = intent.getStringExtra(EXTRA_QUERY).orEmpty()
        if (query.isBlank()) throw BridgeError(translate("intent_ask_contact"))
        val found = accounts(intent.getStringExtra(EXTRA_ACCOUNT))
            .flatMap { repo.contactsFor(it, query) }
            .distinctBy { (it.emails.firstOrNull() ?: it.name).lowercase() }
            .take(limit(intent))
        val text = found.firstOrNull()?.let { c ->
            listOfNotNull(c.name.ifBlank { null }, c.emails.firstOrNull(), c.phones.firstOrNull()).joinToString("\n")
        } ?: translate("intent_no_contacts")
        val json = JsonArray(
            found.map { c ->
                buildJsonObject {
                    put("name", c.name)
                    put("emails", JsonArray(c.emails.map(::JsonPrimitive)))
                    put("phones", JsonArray(c.phones.map(::JsonPrimitive)))
                    put("organization", c.organization)
                }
            },
        ).toString()
        return Result(ok = true, count = found.size, text = text, json = json)
    }

    private suspend fun send(connection: Connection, intent: Intent, request: String): Result {
        if (!connection.send) return Result(ok = false, error = translate("integrations_denied"))
        if (capabilityNeedsPro(Capability.SEND) && !billing.pro.value) {
            return Result(ok = false, error = translate("integrations_needs_pro"))
        }
        val to = intent.getStringExtra(EXTRA_TO).orEmpty().trim()
        if (!to.contains('@')) throw BridgeError(translate("intent_ask_to"))
        val from = accounts(intent.getStringExtra(EXTRA_ACCOUNT)).first()
        val pending = PendingSend(
            id = UUID.randomUUID().toString(),
            token = connection.token,
            request = request,
            account = from.email,
            to = to,
            subject = intent.getStringExtra(EXTRA_SUBJECT).orEmpty(),
            body = intent.getStringExtra(EXTRA_BODY).orEmpty(),
        )
        // The text may come from anywhere (even from a mail an assistant
        // read): unless the user chose otherwise, they confirm each one.
        if (capabilityNeedsConfirmation(Capability.SEND) && connection.askBeforeSend) {
            if (!canNotify()) return Result(ok = false, error = translate("integrations_no_notifications"))
            connections.addPending(pending)
            askToSend(connection, pending)
            return Result(ok = true, status = "waiting", text = translate("integrations_waiting"))
        }
        deliver(pending)
        return Result(ok = true, status = "sent", text = translate("intent_sent"))
    }

    /**
     * Archives or marks the messages named by their `ferrowl://open` links
     * (`link`, several separated by new lines or commas). Ferrowl Pro;
     * confirmed like sending.
     */
    private suspend fun change(connection: Connection, intent: Intent, request: String): Result {
        if (!connection.write) return Result(ok = false, error = translate("integrations_denied"))
        if (capabilityNeedsPro(Capability.WRITE) && !billing.pro.value) {
            return Result(ok = false, error = translate("integrations_needs_pro"))
        }
        val links = intent.getStringExtra(EXTRA_LINK).orEmpty()
            .split('\n', ',').map { it.trim() }.filter { parseLink(it) is FerrowlLink.Open }
        if (links.isEmpty()) throw BridgeError("link?")
        val pending = PendingSend(
            id = UUID.randomUUID().toString(),
            token = connection.token,
            request = request,
            account = "",
            kind = intent.action!!.substringAfterLast('.'),
            links = links,
        )
        if (capabilityNeedsConfirmation(Capability.WRITE) && connection.askBeforeSend) {
            if (!canNotify()) return Result(ok = false, error = translate("integrations_no_notifications"))
            connections.addPending(pending)
            askToSend(connection, pending)
            return Result(ok = true, status = "waiting", text = translate("integrations_waiting"))
        }
        apply(pending)
        return Result(ok = true, status = "done", text = translate("intent_done"))
    }

    /** Carries out a confirmed change, account by account, folder by folder. */
    private suspend fun apply(pending: PendingSend) {
        val opens = pending.links.mapNotNull { parseLink(it) as? FerrowlLink.Open }
        for ((email, mine) in opens.groupBy { it.account.lowercase() }) {
            val account = accounts(email).first()
            val engine = Engine(context.filesDir.resolve("mail").path, Locale.getDefault().language)
            engine.connect(account)
            for ((folder, inFolder) in mine.groupBy { it.folder }) {
                val uids = inFolder.map { it.uid }
                when (pending.kind) {
                    "ARCHIVE" -> engine.archiveMany(folder, uids)
                    "MARK_READ" -> engine.setSeenMany(folder, uids, true)
                    "MARK_UNREAD" -> engine.setSeenMany(folder, uids, false)
                }
            }
            repo.arrived(account.email)
        }
    }

    /** The user answered the notification of a waiting message. */
    suspend fun answer(id: String, allow: Boolean) {
        NotificationManagerCompat.from(context).cancel(id.hashCode())
        val pending = connections.takePending(id) ?: return
        val connection = connections.find(pending.token) ?: return
        val result = if (!allow) {
            Result(ok = false, status = "refused", error = translate("integrations_refused"))
        } else {
            try {
                if (pending.kind == "SEND") {
                    deliver(pending)
                    Result(ok = true, status = "sent", text = translate("intent_sent"))
                } else {
                    apply(pending)
                    Result(ok = true, status = "done", text = translate("intent_done"))
                }
            } catch (e: FerrowlException) {
                Result(ok = false, error = e.message.orEmpty())
            } catch (e: BridgeError) {
                Result(ok = false, error = e.message.orEmpty())
            }
        }
        connections.used(connection, pending.kind, if (result.ok) (result.status ?: "ok") else result.error)
        reply(connection, "com.ferrowl.app.action.${pending.kind}", pending.request, result)
    }

    /** Sends on an engine of its own: the app's may be busy, or on another account. */
    private suspend fun deliver(pending: PendingSend) {
        val account = accounts(pending.account).first()
        val engine = Engine(context.filesDir.resolve("mail").path, Locale.getDefault().language)
        engine.connect(account)
        engine.send(
            Outgoing(
                to = pending.to, cc = "", bcc = "", subject = pending.subject, body = pending.body,
                html = markdownToHtml(pending.body), inReplyTo = null, references = emptyList(),
                attachments = emptyList(), sign = false, encrypt = false,
            ),
        )
    }

    /** New mail arrived for [email] (from a push): tells the connected apps. */
    suspend fun newMail(email: String, peek: InboxPeek) {
        if (peek.fresh.isEmpty()) return
        val readers = connections.list.value.filter { it.read }
        if (readers.isEmpty()) return
        // Without Pro only the first account is used, here too.
        if (runCatching { accounts(email) }.isFailure) return
        val messages = peek.fresh.map { email to it }
        val text = summary(translate("intent_unread_count").replace("{n}", peek.freshCount.toString()), messages)
        for (c in readers) {
            context.sendBroadcast(
                Intent(EVENT_NEW_MAIL).setPackage(c.packageName)
                    .putExtra(EXTRA_ACCOUNT, email)
                    .putExtra("count", peek.freshCount.toInt())
                    .putExtra("unread", peek.unread.toInt())
                    .putExtra("text", text)
                    .putExtra("json", json(messages))
                    .putExtra("from", peek.fresh.first().from.ifBlank { peek.fresh.first().fromAddress })
                    .putExtra("subject", peek.fresh.first().subject),
            )
        }
    }

    private fun reply(connection: Connection, action: String, request: String, r: Result) {
        context.sendBroadcast(
            Intent(EVENT_RESULT).setPackage(connection.packageName)
                .putExtra(EXTRA_REQUEST, request)
                .putExtra("action", action.substringAfterLast('.'))
                .putExtra("ok", r.ok)
                .putExtra("status", r.status ?: if (r.ok) "ok" else "error")
                .putExtra("error", r.error)
                .putExtra("count", r.count)
                .putExtra("text", r.text)
                .putExtra("json", r.json ?: json(r.messages)),
        )
    }

    private fun summary(count: String, messages: List<Pair<String, Header>>): String =
        (listOf(count) + messages.take(3).map { (_, h) -> "${h.from.ifBlank { h.fromAddress }}: ${h.subject}" })
            .joinToString("\n")

    private fun json(messages: List<Pair<String, Header>>): String = JsonArray(
        messages.map { (account, h) ->
            buildJsonObject {
                put("account", account)
                put("from", h.from)
                put("address", h.fromAddress)
                put("subject", h.subject)
                put("date", h.timestamp)
                put("preview", h.preview)
                put("unread", !h.seen)
                put("attachment", h.hasAttachment)
                put("link", JsonPrimitive(messageLink(account, "INBOX", h.uid)))
            }
        },
    ).toString()

    private fun canNotify(): Boolean =
        (Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU ||
            ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED) &&
            NotificationManagerCompat.from(context).areNotificationsEnabled()

    private fun askToSend(connection: Connection, pending: PendingSend) {
        val manager = context.getSystemService(NotificationManager::class.java)
        if (manager.getNotificationChannel(CHANNEL) == null) {
            manager.createNotificationChannel(
                NotificationChannel(CHANNEL, translate("integrations"), NotificationManager.IMPORTANCE_HIGH),
            )
        }
        fun answer(allow: Boolean, code: Int) = PendingIntent.getBroadcast(
            context,
            pending.id.hashCode() * 2 + code,
            Intent(context, BridgeAnswerReceiver::class.java)
                .putExtra(BridgeAnswerReceiver.EXTRA_ID, pending.id)
                .putExtra(BridgeAnswerReceiver.EXTRA_ALLOW, allow),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val subject = pending.subject.ifBlank { translate("no_subject") }
        val sending = pending.kind == "SEND"
        val title = translate(if (sending) "integrations_send_title" else "integrations_change_title")
            .replace("{app}", connection.label)
        val n = pending.links.size.toString()
        val text = when (pending.kind) {
            "ARCHIVE" -> translate("intent_archive_confirm").replace("{n}", n)
            "MARK_READ" -> translate("intent_mark_read_confirm").replace("{n}", n)
            "MARK_UNREAD" -> translate("intent_mark_unread_confirm").replace("{n}", n)
            else -> translate("integrations_send_text").replace("{to}", pending.to).replace("{subject}", subject)
        }
        val notification = NotificationCompat.Builder(context, CHANNEL)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentTitle(title)
            .setContentText(text)
            .setStyle(NotificationCompat.BigTextStyle().bigText(if (sending) text + "\n\n" + pending.body.take(400) else text))
            .setPriority(NotificationCompat.PRIORITY_HIGH)
            .setAutoCancel(false)
            .setOngoing(true)
            .addAction(0, translate(if (sending) "integrations_allow" else "integrations_allow_change"), answer(true, 1))
            .addAction(0, translate("integrations_deny"), answer(false, 0))
            .build()
        try {
            NotificationManagerCompat.from(context).notify(pending.id.hashCode(), notification)
        } catch (_: SecurityException) {
            // Permission withdrawn meanwhile: the message stays unsent.
        }
    }

    companion object {
        const val ACTION_UNREAD = "com.ferrowl.app.action.UNREAD"
        const val ACTION_SEARCH = "com.ferrowl.app.action.SEARCH"
        const val ACTION_SEND = "com.ferrowl.app.action.SEND"
        const val ACTION_AGENDA = "com.ferrowl.app.action.AGENDA"
        const val ACTION_CONTACTS = "com.ferrowl.app.action.CONTACTS"
        const val ACTION_DETAILS = "com.ferrowl.app.action.DETAILS"
        const val ACTION_ARCHIVE = "com.ferrowl.app.action.ARCHIVE"
        const val ACTION_MARK_READ = "com.ferrowl.app.action.MARK_READ"
        const val ACTION_MARK_UNREAD = "com.ferrowl.app.action.MARK_UNREAD"
        const val EVENT_RESULT = "com.ferrowl.app.event.RESULT"
        const val EVENT_NEW_MAIL = "com.ferrowl.app.event.NEW_MAIL"

        const val EXTRA_TOKEN = "token"
        const val EXTRA_REQUEST = "request"
        const val EXTRA_ACCOUNT = "account"
        const val EXTRA_LIMIT = "limit"
        const val EXTRA_QUERY = "query"
        const val EXTRA_TO = "to"
        const val EXTRA_SUBJECT = "subject"
        const val EXTRA_BODY = "body"
        const val EXTRA_DAY = "day"
        const val EXTRA_DAYS = "days"
        const val EXTRA_LINK = "link"

        private const val CHANNEL = "bridge"
        private const val DEFAULT_LIMIT = 10
        private const val MAX_LIMIT = 50
        private const val MAX_TEXT = 20_000
    }
}
