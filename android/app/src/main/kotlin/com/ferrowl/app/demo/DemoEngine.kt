package com.ferrowl.app.demo

import com.ferrowl.app.data.AccountChanged
import com.ferrowl.core.Account
import com.ferrowl.core.CalendarEvent
import com.ferrowl.core.CalendarInfo
import com.ferrowl.core.Contact
import com.ferrowl.core.EventDraft
import com.ferrowl.core.Engine
import com.ferrowl.core.Folder
import com.ferrowl.core.Header
import com.ferrowl.core.HeaderPage
import com.ferrowl.core.InboxPeek
import com.ferrowl.core.InviteReply
import com.ferrowl.core.Message
import com.ferrowl.core.Outgoing
import com.ferrowl.core.RulesOutcome

/**
 * The engine of the demo mode (see [Demo]): a real one on an empty folder
 * of its own, never connected. The mail it shows is [Demo]'s; whatever
 * would reach a server is refused quietly ([AccountChanged] ends the
 * action without a word).
 */
class DemoEngine(dataDir: String, language: String) : Engine(dataDir, language) {
    // ---------------------------------------------------------- the sample mail

    override suspend fun connect(account: Account): List<Folder> = Demo.folders()

    override fun cachedInbox(account: Account): List<Header> = Demo.inbox()

    override fun cachedHeaders(folderId: String): List<Header> = Demo.headers(folderId)

    override suspend fun headers(folderId: String): HeaderPage = HeaderPage(Demo.headers(folderId), null)

    override suspend fun headersForAccount(account: String, folderId: String): HeaderPage = headers(folderId)

    override fun search(folderId: String, query: String): List<Header> = Demo.search(folderId, query)

    override suspend fun unread(folderIds: List<String>): Map<String, UInt> = Demo.unread()

    override suspend fun message(folderId: String, uid: UInt): Message = Demo.message(uid) ?: throw AccountChanged()

    override fun isGmail(): Boolean = false

    override fun agenda(from: Long, to: Long): List<CalendarEvent> = Demo.agenda(from, to)

    override fun calendars(): List<CalendarInfo> = listOf(CalendarInfo("demo:", Demo.text("f_projects"), "#1E5AA8"))

    override fun eventDraft(url: String): EventDraft? = null

    override fun invitationReply(uid: String): InviteReply? = null

    override fun inCalendar(uid: String): Boolean = false

    override suspend fun applySenderRules(folderId: String, headers: List<Header>): List<UInt> = emptyList()

    override suspend fun applySenderRulesForAccount(account: String, folderId: String, headers: List<Header>): List<UInt> =
        emptyList()

    override suspend fun applyRules(folderId: String, headers: List<Header>): RulesOutcome = throw AccountChanged()

    override suspend fun applyRulesForAccount(account: String, folderId: String, headers: List<Header>): RulesOutcome =
        throw AccountChanged()

    override suspend fun syncContacts(): UInt = 0u

    override suspend fun syncCalendar(): UInt = 0u

    override suspend fun indexOlder(folderId: String, more: UInt): UInt? = null

    // ------------------------------------------- refused: they need a server

    override suspend fun test(account: Account): String = throw AccountChanged()

    override suspend fun peekInbox(account: Account, after: UInt, limit: UInt): InboxPeek = throw AccountChanged()

    override suspend fun peekMessage(account: Account, folderId: String, uid: UInt): Message = throw AccountChanged()

    override suspend fun send(message: Outgoing): String? = throw AccountChanged()

    override suspend fun archive(folderId: String, uid: UInt): String = throw AccountChanged()

    override suspend fun archiveMany(folderId: String, uids: List<UInt>): String = throw AccountChanged()

    override suspend fun archiveManyForAccount(account: String, folderId: String, uids: List<UInt>): String =
        throw AccountChanged()

    override suspend fun deleteForever(folderId: String, uid: UInt) = throw AccountChanged()

    override suspend fun deleteForeverMany(folderId: String, uids: List<UInt>) = throw AccountChanged()

    override suspend fun deleteForeverManyForAccount(account: String, folderId: String, uids: List<UInt>) =
        throw AccountChanged()

    override suspend fun moveTo(folderId: String, uid: UInt, targetId: String) = throw AccountChanged()

    override suspend fun moveMany(folderId: String, uids: List<UInt>, targetId: String) = throw AccountChanged()

    override suspend fun moveManyForAccount(account: String, folderId: String, uids: List<UInt>, targetId: String) =
        throw AccountChanged()

    override suspend fun reportSpam(folderId: String, uids: List<UInt>): String = throw AccountChanged()

    override suspend fun reportSpamForAccount(account: String, folderId: String, uids: List<UInt>): String =
        throw AccountChanged()

    override suspend fun notSpam(folderId: String, uids: List<UInt>): String = throw AccountChanged()

    override suspend fun notSpamForAccount(account: String, folderId: String, uids: List<UInt>): String =
        throw AccountChanged()

    override suspend fun setSeen(folderId: String, uid: UInt, seen: Boolean) = throw AccountChanged()

    override suspend fun setSeenMany(folderId: String, uids: List<UInt>, seen: Boolean) = throw AccountChanged()

    override suspend fun setSeenManyForAccount(account: String, folderId: String, uids: List<UInt>, seen: Boolean) =
        throw AccountChanged()

    override suspend fun setFlaggedMany(folderId: String, uids: List<UInt>, flagged: Boolean) = throw AccountChanged()

    override suspend fun setFlaggedManyForAccount(account: String, folderId: String, uids: List<UInt>, flagged: Boolean) =
        throw AccountChanged()

    override suspend fun setLabelMany(folderId: String, uids: List<UInt>, label: String, on: Boolean) =
        throw AccountChanged()

    override suspend fun setLabelManyForAccount(account: String, folderId: String, uids: List<UInt>, label: String, on: Boolean) =
        throw AccountChanged()

    override suspend fun respondInvitation(calendar: String, reply: InviteReply): String = throw AccountChanged()

    override suspend fun removeCancelled(uid: String): String = throw AccountChanged()

    override suspend fun saveEvent(calendarUrl: String, url: String?, draft: EventDraft): String = throw AccountChanged()

    override suspend fun saveEventFor(account: Account, calendarUrl: String, draft: EventDraft): String =
        throw AccountChanged()

    override suspend fun deleteEvent(url: String): String = throw AccountChanged()

    override suspend fun saveContact(contact: Contact): String = throw AccountChanged()

    override suspend fun deleteContact(url: String) = throw AccountChanged()
}
