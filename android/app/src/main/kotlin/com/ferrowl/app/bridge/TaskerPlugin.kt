package com.ferrowl.app.bridge

import android.app.Activity
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import com.ferrowl.app.R
import com.ferrowl.app.ui.theme.FerrowlTheme
import com.ferrowl.core.translate
import dagger.hilt.android.AndroidEntryPoint
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull
import javax.inject.Inject

/**
 * Ferrowl as a Tasker plugin (the Locale/Tasker plugin protocol, also
 * understood by MacroDroid and Automate): Tasker › Action › Plugin ›
 * Ferrowl opens [TaskerPluginActivity] to choose what to do; when the task
 * runs, [TaskerPluginReceiver] does it through [Bridge] (same permissions,
 * Pro and confirmations as the intents) and hands Tasker the results as
 * %fw_ variables.
 */
internal object TaskerPlugin {
    const val EXTRA_BUNDLE = "com.twofortyfouram.locale.intent.extra.BUNDLE"
    const val EXTRA_BLURB = "com.twofortyfouram.locale.intent.extra.BLURB"
    private const val TASKER = "net.dinglisch.android.tasker"
    const val EXTRA_REPLACE_KEYS = "$TASKER.extras.VARIABLE_REPLACE_KEYS"
    const val EXTRA_RELEVANT_VARIABLES = "$TASKER.RELEVANT_VARIABLES"
    const val EXTRA_REQUESTED_TIMEOUT = "$TASKER.extras.REQUESTED_TIMEOUT"
    const val EXTRA_COMPLETION_INTENT = "$TASKER.EXTRA_PLUGIN_COMPLETION_INTENT"
    const val EXTRA_RESULT_CODE = "$TASKER.EXTRA_RESULT_CODE"
    const val EXTRA_VARIABLES = "$TASKER.EXTRA_VARIABLES"

    /** The Bridge action's last word ("UNREAD") and the fields it takes. */
    data class Action(val name: String, val title: String, val fields: List<String>, val permission: String)

    val ACTIONS = listOf(
        Action("UNREAD", "intent_unread_title", listOf("account", "limit"), "read"),
        Action("SEARCH", "intent_search_title", listOf("query", "account", "limit"), "read"),
        Action("DETAILS", "intent_details_title", listOf("link"), "read"),
        Action("AGENDA", "intent_agenda_title", listOf("day", "days", "account"), "read"),
        Action("CONTACTS", "intent_contact_title", listOf("query"), "read"),
        Action("SEND", "intent_send_title", listOf("to", "subject", "body", "account"), "send"),
        Action("ARCHIVE", "intent_archive_title", listOf("link"), "write"),
        Action("MARK_READ", "intent_widget_read", listOf("link"), "write"),
        Action("MARK_UNREAD", "mark_unread", listOf("link"), "write"),
    )

    /** What the action's result becomes in Tasker. */
    val VARIABLES = arrayOf(
        "%fw_ok\nOK\n1 if it worked",
        "%fw_status\nStatus\nok, waiting, sent, done, refused, error",
        "%fw_error\nError\nWhy it did not work",
        "%fw_count\nCount\nHow many messages, events or contacts",
        "%fw_text\nText\nA summary to read out",
        "%fw_json\nJSON\nThe messages, events or contacts",
    )
}

/** The plugin's settings screen, opened by Tasker. */
@AndroidEntryPoint
class TaskerPluginActivity : ComponentActivity() {
    @Inject lateinit var connections: Connections

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val previous = intent.getBundleExtra(TaskerPlugin.EXTRA_BUNDLE)
        // The app that opened us (Tasker…): it is connected to Ferrowl on first use.
        val caller = callingPackage ?: "net.dinglisch.android.taskerm"
        val label = runCatching {
            packageManager.getApplicationLabel(packageManager.getApplicationInfo(caller, 0)).toString()
        }.getOrDefault(caller)
        val connection = connections.forPackage(caller, label)
        setContent {
            FerrowlTheme {
                PluginScreen(previous, connection, onDone = { bundle, blurb -> finishWith(bundle, blurb) })
            }
        }
    }

    private fun finishWith(bundle: Bundle, blurb: String) {
        setResult(
            Activity.RESULT_OK,
            Intent()
                .putExtra(TaskerPlugin.EXTRA_BUNDLE, bundle)
                .putExtra(TaskerPlugin.EXTRA_BLURB, blurb)
                .putExtra(TaskerPlugin.EXTRA_RELEVANT_VARIABLES, TaskerPlugin.VARIABLES)
                // Tasker waits for the answer (a send may wait for the user's OK).
                .putExtra(TaskerPlugin.EXTRA_REQUESTED_TIMEOUT, 60_000),
        )
        finish()
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun PluginScreen(previous: Bundle?, connection: Connection, onDone: (Bundle, String) -> Unit) {
    var chosen by remember { mutableStateOf(TaskerPlugin.ACTIONS.firstOrNull { it.name == previous?.getString("action") } ?: TaskerPlugin.ACTIONS[0]) }
    val values = remember { mutableStateMapOf<String, String>().apply { previous?.keySet()?.forEach { k -> previous.getString(k)?.let { put(k, it) } } } }
    Scaffold(topBar = { TopAppBar(title = { Text("Ferrowl") }) }) { padding ->
        Column(
            Modifier.fillMaxSize().padding(padding).verticalScroll(rememberScrollState()).padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            TaskerPlugin.ACTIONS.forEach { a ->
                ListItem(
                    headlineContent = { Text(translate(a.title)) },
                    leadingContent = { RadioButton(selected = chosen == a, onClick = { chosen = a }) },
                    modifier = Modifier.fillMaxWidth().selectable(selected = chosen == a, onClick = { chosen = a }),
                )
            }
            chosen.fields.forEach { field ->
                OutlinedTextField(
                    value = values[field].orEmpty(),
                    onValueChange = { values[field] = it },
                    label = { Text(field) },
                    placeholder = { Text("%var") },
                    singleLine = field != "body" && field != "link",
                    modifier = Modifier.fillMaxWidth(),
                )
            }
            // Sending and changing mail are allowed in Ferrowl's settings.
            val allowed = when (chosen.permission) {
                "send" -> connection.send
                "write" -> connection.write
                else -> connection.read
            }
            if (!allowed) {
                Text(stringResource(R.string.integrations_denied), color = MaterialTheme.colorScheme.error)
            }
            Button(onClick = {
                val bundle = Bundle().apply {
                    putString("action", chosen.name)
                    putString("token", connection.token)
                    chosen.fields.forEach { putString(it, values[it].orEmpty()) }
                    // Tasker fills in the %variables of these fields before running.
                    putString(TaskerPlugin.EXTRA_REPLACE_KEYS, chosen.fields.joinToString(" "))
                }
                val detail = chosen.fields.mapNotNull { values[it]?.takeIf(String::isNotBlank) }.joinToString(" · ")
                onDone(bundle, listOf(translate(chosen.title), detail).filter { it.isNotBlank() }.joinToString(": "))
            }) { Text(stringResource(R.string.rules_save)) }
        }
    }
}

/** Runs the plugin's action when a Tasker task gets to it. */
@AndroidEntryPoint
class TaskerPluginReceiver : BroadcastReceiver() {
    @Inject lateinit var bridge: Bridge

    override fun onReceive(context: Context, intent: Intent) {
        val bundle = intent.getBundleExtra(TaskerPlugin.EXTRA_BUNDLE) ?: return
        val action = bundle.getString("action") ?: return
        val request = Intent("com.ferrowl.app.action.$action").apply {
            bundle.keySet().filter { it != "action" && !it.startsWith("net.dinglisch") }.forEach { k ->
                bundle.getString(k)?.let { putExtra(k, it) }
            }
        }
        val done = goAsync()
        CoroutineScope(SupervisorJob() + Dispatchers.IO).launch {
            try {
                val result = withTimeoutOrNull(50_000L) { bridge.run(request)?.second }
                finish(context, intent, result)
            } finally {
                done.finish()
            }
        }
    }

    /** Hands Tasker the result (the plugin protocol's completion intent). */
    private fun finish(context: Context, fired: Intent, r: Bridge.Result?) {
        val uri = fired.getStringExtra(TaskerPlugin.EXTRA_COMPLETION_INTENT) ?: return
        val vars = Bundle().apply {
            putString("%fw_ok", if (r?.ok == true) "1" else "0")
            putString("%fw_status", r?.status ?: if (r?.ok == true) "ok" else "error")
            putString("%fw_error", r?.error ?: translate("integrations_denied"))
            putString("%fw_count", (r?.count ?: 0).toString())
            putString("%fw_text", r?.text.orEmpty())
            putString("%fw_json", r?.json ?: "[]")
        }
        val completion = runCatching { Intent.parseUri(uri, Intent.URI_INTENT_SCHEME) }.getOrNull() ?: return
        completion.putExtra(TaskerPlugin.EXTRA_RESULT_CODE, if (r?.ok == true) Activity.RESULT_OK else Activity.RESULT_FIRST_USER)
        completion.putExtra(TaskerPlugin.EXTRA_VARIABLES, vars)
        context.sendBroadcast(completion)
    }
}
