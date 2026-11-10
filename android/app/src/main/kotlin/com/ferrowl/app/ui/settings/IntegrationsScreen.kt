package com.ferrowl.app.ui.settings

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.pm.PackageManager
import android.text.format.DateUtils
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import androidx.hilt.navigation.compose.hiltViewModel
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.ferrowl.app.R
import com.ferrowl.app.bridge.Connection
import com.ferrowl.app.bridge.Connections
import com.ferrowl.app.data.Billing
import com.ferrowl.app.ui.notice.LocalNotices
import dagger.hilt.android.lifecycle.HiltViewModel
import javax.inject.Inject

@HiltViewModel
class IntegrationsViewModel @Inject constructor(
    private val connections: Connections,
    billing: Billing,
) : ViewModel() {
    val list = connections.list
    val log = connections.log
    val pro = billing.pro

    fun add(packageName: String, label: String): Connection = connections.add(packageName, label)

    fun update(connection: Connection) = connections.update(connection)

    fun remove(connection: Connection) = connections.remove(connection.token)
}

/**
 * Settings → Integrations: automation apps (Tasker, MacroDroid, Automate…)
 * connected to Ferrowl, what each may do, their keys, how to use them, and
 * what they did lately.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun IntegrationsScreen(onBack: () -> Unit, vm: IntegrationsViewModel = hiltViewModel()) {
    val list by vm.list.collectAsStateWithLifecycle()
    val log by vm.log.collectAsStateWithLifecycle()
    val pro by vm.pro.collectAsStateWithLifecycle()
    val context = LocalContext.current
    val notices = LocalNotices.current
    var adding by remember { mutableStateOf(false) }
    var removing by remember { mutableStateOf<Connection?>(null) }
    val copied = stringResource(R.string.integrations_copied)
    fun copy(c: Connection) {
        val clipboard = context.getSystemService(ClipboardManager::class.java)
        clipboard.setPrimaryClip(ClipData.newPlainText("Ferrowl", c.token))
        notices.info(copied)
    }
    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(stringResource(R.string.integrations)) },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(R.string.settings))
                    }
                },
            )
        },
    ) { padding ->
        Column(
            Modifier.fillMaxSize().padding(padding).verticalScroll(rememberScrollState()).padding(horizontal = 16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Text(stringResource(R.string.integrations_desc), color = MaterialTheme.colorScheme.onSurfaceVariant)
            if (list.isEmpty()) {
                Text(stringResource(R.string.integrations_none), style = MaterialTheme.typography.bodyLarge)
            }
            list.forEach { c ->
                ConnectionCard(c, pro, onChange = vm::update, onCopy = { copy(c) }, onRemove = { removing = c })
            }
            Button(onClick = { adding = true }) { Text(stringResource(R.string.integrations_add)) }

            HorizontalDivider(Modifier.padding(vertical = 8.dp))
            Text(stringResource(R.string.integrations_how), style = MaterialTheme.typography.titleSmall)
            Text(stringResource(R.string.integrations_help), style = MaterialTheme.typography.bodyMedium)
            Text(
                """
                Send Intent
                  Action: com.ferrowl.app.action.UNREAD
                          (SEARCH: query; SEND: to, subject, body;
                           AGENDA: day, days; CONTACTS: query;
                           ARCHIVE, MARK_READ, MARK_UNREAD: link)
                  Extra: token:<key>
                  Package: com.ferrowl.app
                  Target: Broadcast Receiver

                Event › Intent Received
                  com.ferrowl.app.event.RESULT
                    %ok %count %text %json %error %request
                  com.ferrowl.app.event.NEW_MAIL
                    %account %count %from %subject %text %json
                """.trimIndent(),
                fontFamily = FontFamily.Monospace,
                style = MaterialTheme.typography.bodySmall,
            )

            if (log.isNotEmpty()) {
                HorizontalDivider(Modifier.padding(vertical = 8.dp))
                Text(stringResource(R.string.integrations_log), style = MaterialTheme.typography.titleSmall)
                log.take(20).forEach { e ->
                    Text(
                        "${DateUtils.getRelativeTimeSpanString(e.time)} · ${e.label} · ${e.action} · ${e.outcome}",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
            Spacer(Modifier.height(24.dp))
        }
    }
    if (adding) {
        AddDialog(
            context = context,
            onDismiss = { adding = false },
            onAdd = { pkg, label ->
                adding = false
                copy(vm.add(pkg, label))
            },
        )
    }
    removing?.let { c ->
        AlertDialog(
            onDismissRequest = { removing = null },
            title = { Text(stringResource(R.string.integrations_remove)) },
            text = { Text(c.label) },
            confirmButton = {
                TextButton(onClick = {
                    vm.remove(c)
                    removing = null
                }) { Text(stringResource(R.string.integrations_remove), color = MaterialTheme.colorScheme.error) }
            },
            dismissButton = { TextButton(onClick = { removing = null }) { Text(stringResource(R.string.cancel)) } },
        )
    }
}

@Composable
private fun ConnectionCard(
    c: Connection,
    pro: Boolean,
    onChange: (Connection) -> Unit,
    onCopy: () -> Unit,
    onRemove: () -> Unit,
) {
    Card(Modifier.fillMaxWidth()) {
        Column(Modifier.padding(vertical = 8.dp)) {
            ListItem(
                headlineContent = { Text(c.label) },
                supportingContent = {
                    Column {
                        Text(c.packageName)
                        if (c.lastUsed > 0) {
                            Text(
                                stringResource(R.string.integrations_last_used)
                                    .replace("{when}", DateUtils.getRelativeTimeSpanString(c.lastUsed).toString()),
                            )
                        }
                    }
                },
            )
            Toggle(stringResource(R.string.integrations_read), c.read) { onChange(c.copy(read = it)) }
            Toggle(
                stringResource(R.string.integrations_send),
                c.send && pro,
                enabled = pro,
                note = if (pro) null else stringResource(R.string.integrations_needs_pro),
            ) { onChange(c.copy(send = it)) }
            Toggle(
                stringResource(R.string.integrations_write),
                c.write && pro,
                enabled = pro,
                note = if (pro) null else stringResource(R.string.integrations_needs_pro),
            ) { onChange(c.copy(write = it)) }
            if ((c.send || c.write) && pro) {
                Toggle(stringResource(R.string.integrations_ask), c.askBeforeSend) { onChange(c.copy(askBeforeSend = it)) }
            }
            Row(Modifier.padding(horizontal = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                Text(
                    "${stringResource(R.string.integrations_token)}: ${c.token.take(6)}…",
                    fontFamily = FontFamily.Monospace,
                    modifier = Modifier.weight(1f).padding(start = 8.dp),
                )
                TextButton(onClick = onCopy) { Text(stringResource(R.string.integrations_copy)) }
                TextButton(onClick = onRemove) {
                    Text(stringResource(R.string.integrations_remove), color = MaterialTheme.colorScheme.error)
                }
            }
        }
    }
}

@Composable
private fun Toggle(text: String, on: Boolean, enabled: Boolean = true, note: String? = null, onChange: (Boolean) -> Unit) {
    ListItem(
        headlineContent = { Text(text) },
        supportingContent = note?.let { { Text(it) } },
        trailingContent = { Switch(checked = on, enabled = enabled, onCheckedChange = onChange) },
    )
}

/** Picks the app to connect: the automation apps installed, or any by package name. */
@Composable
private fun AddDialog(context: Context, onDismiss: () -> Unit, onAdd: (String, String) -> Unit) {
    val installed = remember {
        Connections.KNOWN.mapNotNull { (pkg, name) ->
            try {
                val info = context.packageManager.getApplicationInfo(pkg, 0)
                pkg to (context.packageManager.getApplicationLabel(info).toString().ifBlank { name })
            } catch (_: PackageManager.NameNotFoundException) {
                null
            }
        }
    }
    var chosen by rememberSaveable { mutableStateOf(installed.firstOrNull()?.first ?: "") }
    var other by rememberSaveable { mutableStateOf("") }
    val otherChosen = chosen.isEmpty()
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.integrations_add)) },
        text = {
            Column {
                installed.forEach { (pkg, label) ->
                    Row(
                        Modifier.fillMaxWidth().selectable(selected = chosen == pkg, onClick = { chosen = pkg }),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        RadioButton(selected = chosen == pkg, onClick = { chosen = pkg })
                        Text(label)
                    }
                }
                Row(
                    Modifier.fillMaxWidth().clickable { chosen = "" },
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    RadioButton(selected = otherChosen, onClick = { chosen = "" })
                    Text(stringResource(R.string.integrations_other))
                }
                if (otherChosen) {
                    OutlinedTextField(
                        value = other,
                        onValueChange = { other = it.trim() },
                        placeholder = { Text("com.example.app") },
                        singleLine = true,
                    )
                }
            }
        },
        confirmButton = {
            val pkg = if (otherChosen) other else chosen
            TextButton(
                enabled = pkg.matches(Regex("""[A-Za-z][\w]*(\.[A-Za-z][\w]*)+""")),
                onClick = {
                    val label = installed.firstOrNull { it.first == pkg }?.second ?: pkg
                    onAdd(pkg, label)
                },
            ) { Text(stringResource(R.string.integrations_connect)) }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.cancel)) } },
    )
}
