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
import com.ferrowl.core.Header
import com.ferrowl.core.InboxPeek
import com.ferrowl.core.Outgoing
import com.ferrowl.core.capabilityNeedsConfirmation
import com.ferrowl.core.capabilityNeedsPro
import com.ferrowl.core.markdownToHtml
import com.ferrowl.core.messageLink
import com.ferrowl.core.translate
import dagger.hilt.android.qualifiers.ApplicationContext
import kotlinx.coroutines.flow.first
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import java.util.Locale
import java.util.UUID
import javax.inject.Inject
import javax.inject.Singleton

/**
 * Ferrowl Bridge on Android: what automation apps (Tasker, MacroDroid,
 * Automate…) may ask of Ferrowl through intents, and the events they get.
 *
 * Requests are broadcasts to Ferrowl ([BridgeReceiver]) with the app's key
 * in the `token` extra and an optional `request` id echoed in the answer:
 * - [ACTION_UNREAD] (`account`, `limit`): unread mail from the server;
 * - [ACTION_SEARCH] (`query`, `account`, `limit`): mail stored on the phone;
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
                ACTION_SEND -> send(connection, intent, request)
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

    /** The user answered the notification of a waiting message. */
    suspend fun answer(id: String, allow: Boolean) {
        NotificationManagerCompat.from(context).cancel(id.hashCode())
        val pending = connections.takePending(id) ?: return
        val connection = connections.find(pending.token) ?: return
        val result = if (!allow) {
            Result(ok = false, status = "refused", error = translate("integrations_refused"))
        } else {
            try {
                deliver(pending)
                Result(ok = true, status = "sent", text = translate("intent_sent"))
            } catch (e: FerrowlException) {
                Result(ok = false, error = e.message.orEmpty())
            } catch (e: BridgeError) {
                Result(ok = false, error = e.message.orEmpty())
            }
        }
        connections.used(connection, "SEND", if (result.ok) "sent" else result.error)
        reply(connection, ACTION_SEND, pending.request, result)
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
                .putExtra("json", json(r.messages)),
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
        val notification = NotificationCompat.Builder(context, CHANNEL)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentTitle(translate("integrations_send_title").replace("{app}", connection.label))
            .setContentText(translate("integrations_send_text").replace("{to}", pending.to).replace("{subject}", subject))
            .setStyle(
                NotificationCompat.BigTextStyle().bigText(
                    translate("integrations_send_text").replace("{to}", pending.to).replace("{subject}", subject) +
                        "\n\n" + pending.body.take(400),
                ),
            )
            .setPriority(NotificationCompat.PRIORITY_HIGH)
            .setAutoCancel(false)
            .setOngoing(true)
            .addAction(0, translate("integrations_allow"), answer(true, 1))
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

        private const val CHANNEL = "bridge"
        private const val DEFAULT_LIMIT = 10
        private const val MAX_LIMIT = 50
    }
}
