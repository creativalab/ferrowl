package com.ferrowl.app.ui.settings

import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
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
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
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
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import androidx.hilt.navigation.compose.hiltViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.ferrowl.app.R
import com.ferrowl.app.ui.notice.LocalBilling
import com.ferrowl.app.ui.notice.ReportError
import com.ferrowl.core.translate

/** App settings (for now: how the message list is grouped). */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SettingsScreen(
    onBack: () -> Unit,
    onNoAccount: () -> Unit,
    onIntegrations: () -> Unit = {},
    vm: SettingsViewModel = hiltViewModel(),
) {
    val threaded by vm.threaded.collectAsStateWithLifecycle()
    val pushOn by vm.pushEnabled.collectAsStateWithLifecycle()
    val pushUnavailable by vm.pushUnavailable.collectAsStateWithLifecycle()
    val pushAvailable = pushUnavailable == null
    val activeEmail by vm.activeEmail.collectAsStateWithLifecycle()
    val noAccount by vm.noAccount.collectAsStateWithLifecycle()
    var confirmRemove by remember { mutableStateOf(false) }
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
            val billing = LocalBilling.current
            val subscribed by billing.subscribed.collectAsStateWithLifecycle()
            if (subscribed) {
                val context = LocalContext.current
                HorizontalDivider(Modifier.padding(vertical = 16.dp))
                ListItem(
                    headlineContent = { Text(stringResource(R.string.pro_title)) },
                    supportingContent = { Text(stringResource(R.string.pro_manage)) },
                    modifier = Modifier.clickable { billing.manage(context) },
                )
            }
            HorizontalDivider(Modifier.padding(vertical = 16.dp))
            ListItem(
                headlineContent = { Text(stringResource(R.string.remove_account)) },
                supportingContent = { Text(activeEmail) },
                modifier = Modifier.clickable { confirmRemove = true },
            )
            Spacer(Modifier.height(24.dp))
        }
    }
    if (confirmRemove) {
        AlertDialog(
            onDismissRequest = { confirmRemove = false },
            title = { Text(stringResource(R.string.remove_account)) },
            text = { Text(stringResource(R.string.confirm_remove_account)) },
            confirmButton = {
                TextButton(onClick = {
                    confirmRemove = false
                    vm.removeActiveAccount()
                }) { Text(stringResource(R.string.remove_account), color = MaterialTheme.colorScheme.error) }
            },
            dismissButton = { TextButton(onClick = { confirmRemove = false }) { Text(stringResource(R.string.cancel)) } },
        )
    }
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
