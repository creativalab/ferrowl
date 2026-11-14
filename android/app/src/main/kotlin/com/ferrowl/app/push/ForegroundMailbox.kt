package com.ferrowl.app.push

/**
 * Whether the app is on screen, and its account in use: that account's new
 * mail needs no pop-up, its list shows it at once (see
 * [MailNotifier.announce]), as iOS does for the account on screen.
 */
object ForegroundMailbox {
    @Volatile var foreground = false

    @Volatile var active: String? = null

    fun shows(account: String): Boolean = foreground && account.equals(active, ignoreCase = true)
}
