package com.ferrowl.app.ui.settings

import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import androidx.core.net.toUri
import androidx.hilt.navigation.compose.hiltViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.ferrowl.app.BuildConfig
import com.ferrowl.app.R
import com.ferrowl.app.ui.notice.LocalBilling
import com.ferrowl.app.ui.notice.ReportError
import com.ferrowl.app.ui.theme.THEME_IDS
import com.ferrowl.app.ui.theme.themeAccent
import com.ferrowl.core.Account
import com.ferrowl.core.OAuthProvider
import com.ferrowl.core.Protocol
import com.ferrowl.core.translate

/** App settings (for now: how the message list is grouped). */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SettingsScreen(
    onBack: () -> Unit,
    onNoAccount: () -> Unit,
    onIntegrations: () -> Unit = {},
    onRules: () -> Unit = {},
    onAddAccount: () -> Unit = {},
    vm: SettingsViewModel = hiltViewModel(),
) {
    val threaded by vm.threaded.collectAsStateWithLifecycle()
    val pushOn by vm.pushEnabled.collectAsStateWithLifecycle()
    val pushUnavailable by vm.pushUnavailable.collectAsStateWithLifecycle()
    val pushAvailable = pushUnavailable == null
    val activeEmail by vm.activeEmail.collectAsStateWithLifecycle()
    val noAccount by vm.noAccount.collectAsStateWithLifecycle()
    var detail by remember { mutableStateOf<Account?>(null) }
    var removing by remember { mutableStateOf<String?>(null) }
    LaunchedEffect(noAccount) { if (noAccount) onNoAccount() }
    val pushBusy by vm.pushBusy.collectAsStateWithLifecycle()
    val pushError by vm.pushError.collectAsStateWithLifecycle()
    ReportError(pushError, vm::pushErrorShown)
    // Android 13+ asks the user before an app may show notifications.
    val askPermission = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
        if (granted) vm.setPush(true)
    }
    val context = LocalContext.current
    fun turnOn() {
        val needsAsking = Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
            ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
        if (needsAsking) askPermission.launch(Manifest.permission.POST_NOTIFICATIONS) else vm.setPush(true)
    }
    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(stringResource(R.string.settings)) },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(R.string.back_to_mail))
                    }
                },
            )
        },
    ) { padding ->
        Column(Modifier.fillMaxSize().padding(padding).verticalScroll(rememberScrollState())) {
            // Accounts, as on iOS: each opens its settings.
            val accounts by vm.accounts.collectAsStateWithLifecycle()
            SectionTitle(stringResource(R.string.section_accounts))
            accounts.forEach { account ->
                ListItem(
                    headlineContent = { Text(account.displayName.ifBlank { account.email }) },
                    supportingContent = { Text(account.email) },
                    trailingContent = {
                        if (account.email.equals(activeEmail, ignoreCase = true)) {
                            Icon(Icons.Filled.Check, contentDescription = null, tint = MaterialTheme.colorScheme.primary)
                        }
                    },
                    modifier = Modifier.clickable { detail = account },
                )
            }
            // After the first account Ferrowl Pro is needed (as in the drawer):
            // without it, the subscription is offered instead.
            val hasPro by LocalBilling.current.pro.collectAsStateWithLifecycle()
            val buyBilling = LocalBilling.current
            val noPro = stringResource(R.string.pro_unavailable)
            val addNotices = com.ferrowl.app.ui.notice.LocalNotices.current
            ListItem(
                headlineContent = { Text(stringResource(R.string.add_account)) },
                leadingContent = { Icon(Icons.Filled.Add, contentDescription = null) },
                modifier = Modifier.clickable {
                    if (hasPro || accounts.isEmpty()) {
                        onAddAccount()
                    } else {
                        val activity = context as? android.app.Activity
                        if (activity == null || !buyBilling.buy(activity)) addNotices.error(noPro)
                    }
                },
            )
            HorizontalDivider(Modifier.padding(vertical = 16.dp))
            // Ferrowl Pro: subscribe, or manage the subscription.
            val billing = LocalBilling.current
            val pro by billing.pro.collectAsStateWithLifecycle()
            val subscribed by billing.subscribed.collectAsStateWithLifecycle()
            val context = LocalContext.current
            SectionTitle(stringResource(R.string.pro_title))
            if (pro) {
                ListItem(
                    headlineContent = { Text(stringResource(R.string.pro_thanks)) },
                    leadingContent = { Icon(Icons.Filled.CheckCircle, contentDescription = null, tint = MaterialTheme.colorScheme.primary) },
                )
            }
            if (subscribed) {
                ListItem(
                    headlineContent = { Text(stringResource(R.string.pro_manage)) },
                    modifier = Modifier.clickable { billing.manage(context) },
                )
            } else if (!pro) {
                val activity = context as? android.app.Activity
                val unavailable = stringResource(R.string.pro_unavailable)
                val notices = com.ferrowl.app.ui.notice.LocalNotices.current
                ListItem(
                    headlineContent = { Text(stringResource(R.string.pro_subscribe)) },
                    supportingContent = { Text(stringResource(R.string.pro_text).replace("{price}", billing.price.collectAsStateWithLifecycle().value ?: "…")) },
                    modifier = Modifier.clickable { if (activity == null || !billing.buy(activity)) notices.error(unavailable) },
                )
            }
            ListItem(
                headlineContent = { Text(stringResource(R.string.pro_restore)) },
                modifier = Modifier.clickable { billing.start() },
            )
            HorizontalDivider(Modifier.padding(vertical = 16.dp))
            SectionTitle(stringResource(R.string.section_general))
            ListItem(
                headlineContent = { Text(stringResource(R.string.list_view)) },
                supportingContent = { Text(stringResource(R.string.list_view_desc)) },
            )
            SingleChoiceSegmentedButtonRow(Modifier.padding(horizontal = 16.dp)) {
                SegmentedButton(
                    selected = threaded,
                    onClick = { vm.setThreaded(true) },
                    shape = SegmentedButtonDefaults.itemShape(index = 0, count = 2),
                ) { Text(stringResource(R.string.view_threads)) }
                SegmentedButton(
                    selected = !threaded,
                    onClick = { vm.setThreaded(false) },
                    shape = SegmentedButtonDefaults.itemShape(index = 1, count = 2),
                ) { Text(stringResource(R.string.view_messages)) }
            }
            HorizontalDivider(Modifier.padding(vertical = 16.dp))
            ListItem(
                headlineContent = { Text(stringResource(R.string.push_title)) },
                supportingContent = {
                    Text(pushUnavailable?.let { translate(it) } ?: stringResource(R.string.push_desc))
                },
                trailingContent = {
                    Switch(
                        checked = pushOn,
                        enabled = pushAvailable && !pushBusy,
                        onCheckedChange = { on -> if (on) turnOn() else vm.setPush(false) },
                    )
                },
            )
            if (pushBusy) LinearProgressIndicator(Modifier.fillMaxWidth().padding(horizontal = 16.dp))
            val remoteImages by vm.remoteImages.collectAsStateWithLifecycle()
            ListItem(
                headlineContent = { Text(stringResource(R.string.remote_images)) },
                supportingContent = { Text(stringResource(R.string.remote_images_desc)) },
                trailingContent = { Switch(checked = remoteImages, onCheckedChange = vm::setRemoteImages) },
            )
            HorizontalDivider(Modifier.padding(vertical = 16.dp))
            // The user's rules for new mail.
            ListItem(
                headlineContent = { Text(stringResource(R.string.rules_title)) },
                supportingContent = { Text(stringResource(R.string.rules_desc)) },
                modifier = Modifier.clickable(onClick = onRules),
            )
            HorizontalDivider(Modifier.padding(vertical = 16.dp))
            // Automation apps (Tasker…) connected through Ferrowl Bridge.
            ListItem(
                headlineContent = { Text(stringResource(R.string.integrations)) },
                supportingContent = { Text(stringResource(R.string.integrations_desc)) },
                modifier = Modifier.clickable(onClick = onIntegrations),
            )
            HorizontalDivider(Modifier.padding(vertical = 16.dp))
            val blocked by vm.blocked.collectAsStateWithLifecycle()
            val allowed by vm.allowed.collectAsStateWithLifecycle()
            ListItem(
                headlineContent = { Text(stringResource(R.string.senders_title)) },
                supportingContent = { Text(stringResource(R.string.senders_desc)) },
            )
            SenderList(stringResource(R.string.senders_blocked), blocked, onAdd = { vm.addSender(it, false) }, onRemove = vm::removeSender)
            SenderList(stringResource(R.string.senders_allowed), allowed, onAdd = { vm.addSender(it, true) }, onRemove = vm::removeSender)
            // Color themes of design/themes.json, as on desktop and iOS.
            HorizontalDivider(Modifier.padding(vertical = 16.dp))
            val theme by vm.theme.collectAsStateWithLifecycle()
            SectionTitle(stringResource(R.string.color_theme))
            THEME_IDS.forEach { id ->
                ListItem(
                    headlineContent = { Text(translate("theme_$id")) },
                    leadingContent = { RadioButton(selected = theme == id, onClick = { vm.setTheme(id) }) },
                    trailingContent = {
                        if (id != "auto") Box(Modifier.size(20.dp).clip(CircleShape).background(themeAccent(id)))
                    },
                    modifier = Modifier.clickable { vm.setTheme(id) },
                )
            }
            HorizontalDivider(Modifier.padding(vertical = 16.dp))
            ListItem(headlineContent = { Text("Ferrowl") }, trailingContent = { Text(BuildConfig.VERSION_NAME) })
            val context2 = LocalContext.current
            ListItem(
                headlineContent = { Text("support@ferrowl.com") },
                modifier = Modifier.clickable {
                    runCatching {
                        context2.startActivity(android.content.Intent(android.content.Intent.ACTION_SENDTO, "mailto:support@ferrowl.com".toUri()))
                    }
                },
            )
            Spacer(Modifier.height(24.dp))
        }
    }
    detail?.let { account ->
        AccountDialog(
            account = account,
            onDismiss = { detail = null },
            onSave = {
                vm.updateAccount(it)
                detail = null
            },
            onRemove = {
                detail = null
                removing = account.email
            },
        )
    }
    removing?.let { email ->
        AlertDialog(
            onDismissRequest = { removing = null },
            title = { Text(stringResource(R.string.remove_account)) },
            text = { Text(stringResource(R.string.confirm_remove_account) + "\n\n" + email) },
            confirmButton = {
                TextButton(onClick = {
                    removing = null
                    vm.removeAccount(email)
                }) { Text(stringResource(R.string.remove_account), color = MaterialTheme.colorScheme.error) }
            },
            dismissButton = { TextButton(onClick = { removing = null }) { Text(stringResource(R.string.cancel)) } },
        )
    }
}

@Composable
private fun SectionTitle(text: String) {
    Text(
        text,
        style = MaterialTheme.typography.titleSmall,
        color = MaterialTheme.colorScheme.primary,
        modifier = Modifier.padding(start = 16.dp, top = 8.dp, bottom = 4.dp),
    )
}

/** An account's settings, as on iOS: its name, its servers, the contacts and calendar servers, removing it. */
@Composable
private fun AccountDialog(account: Account, onDismiss: () -> Unit, onSave: (Account) -> Unit, onRemove: () -> Unit) {
    var name by rememberSaveable { mutableStateOf(account.displayName) }
    var carddav by rememberSaveable { mutableStateOf(account.carddavUrl) }
    var caldav by rememberSaveable { mutableStateOf(account.caldavUrl) }
    var davUser by rememberSaveable { mutableStateOf(account.davUsername) }
    var davPassword by remember { mutableStateOf(account.davPassword) }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.account_settings)) },
        text = {
            Column(Modifier.verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(account.email, style = MaterialTheme.typography.bodyMedium)
                OutlinedTextField(
                    value = name,
                    onValueChange = { name = it },
                    label = { Text(stringResource(R.string.display_name)) },
                    singleLine = true,
                )
                val oauth = account.oauth
                if (oauth != null) {
                    Text(
                        translate("oauth_signed_in").replace("{provider}", if (oauth == OAuthProvider.MICROSOFT) "Microsoft" else "Google"),
                        style = MaterialTheme.typography.bodySmall,
                    )
                } else {
                    Text(
                        stringResource(if (account.protocol == Protocol.POP3) R.string.pop3_server else R.string.imap_server) +
                            ": ${account.incomingHost}:${account.incomingPort}",
                        style = MaterialTheme.typography.bodySmall,
                    )
                    Text(
                        stringResource(R.string.smtp_server) + ": ${account.smtpHost}:${account.smtpPort}",
                        style = MaterialTheme.typography.bodySmall,
                    )
                }
                OutlinedTextField(
                    value = carddav,
                    onValueChange = { carddav = it.trim() },
                    label = { Text(stringResource(R.string.carddav_url)) },
                    placeholder = { Text("https://") },
                    singleLine = true,
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri),
                )
                OutlinedTextField(
                    value = caldav,
                    onValueChange = { caldav = it.trim() },
                    label = { Text(stringResource(R.string.caldav_url)) },
                    placeholder = { Text("https://") },
                    supportingText = { Text(stringResource(R.string.carddav_url_hint)) },
                    singleLine = true,
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri),
                )
                Text(stringResource(R.string.dav_credentials), style = MaterialTheme.typography.titleSmall)
                OutlinedTextField(
                    value = davUser,
                    onValueChange = { davUser = it.trim() },
                    label = { Text(stringResource(R.string.dav_username)) },
                    singleLine = true,
                )
                OutlinedTextField(
                    value = davPassword,
                    onValueChange = { davPassword = it },
                    label = { Text(stringResource(R.string.dav_password)) },
                    supportingText = { Text(stringResource(R.string.dav_credentials_hint)) },
                    singleLine = true,
                    visualTransformation = PasswordVisualTransformation(),
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password),
                )
                TextButton(onClick = onRemove) {
                    Text(stringResource(R.string.remove_account), color = MaterialTheme.colorScheme.error)
                }
            }
        },
        confirmButton = {
            TextButton(onClick = {
                onSave(
                    account.copy(
                        displayName = name.trim(),
                        carddavUrl = carddav,
                        caldavUrl = caldav,
                        davUsername = davUser,
                        davPassword = if (davUser.isEmpty()) "" else davPassword,
                    ),
                )
            }) { Text(stringResource(R.string.rules_save)) }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.cancel)) } },
    )
}

/** One sender list: its addresses (each removable) and a box to add one. */
@Composable
private fun SenderList(title: String, entries: List<String>, onAdd: (String) -> Unit, onRemove: (String) -> Unit) {
    var typed by rememberSaveable { mutableStateOf("") }
    Column(Modifier.padding(horizontal = 16.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Text(title, style = MaterialTheme.typography.titleSmall)
        if (entries.isEmpty()) {
            Text(stringResource(R.string.senders_none), color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        entries.forEach { address ->
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(address, modifier = Modifier.weight(1f))
                IconButton(onClick = { onRemove(address) }) {
                    Icon(Icons.Filled.Close, contentDescription = stringResource(R.string.senders_remove))
                }
            }
        }
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            OutlinedTextField(
                value = typed,
                onValueChange = { typed = it },
                placeholder = { Text(stringResource(R.string.senders_placeholder)) },
                singleLine = true,
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Email),
                modifier = Modifier.weight(1f),
            )
            TextButton(
                onClick = {
                    onAdd(typed)
                    typed = ""
                },
                enabled = typed.contains('@'),
            ) { Text(stringResource(R.string.senders_add)) }
        }
    }
}
