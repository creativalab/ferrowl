package com.ferrowl.app.ui.settings

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
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
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
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
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.hilt.navigation.compose.hiltViewModel
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewModelScope
import com.ferrowl.app.R
import com.ferrowl.app.data.Billing
import com.ferrowl.app.data.MailRepository
import com.ferrowl.core.Folder
import com.ferrowl.core.Header
import com.ferrowl.core.Rule
import com.ferrowl.core.RuleAction
import dagger.hilt.android.lifecycle.HiltViewModel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import java.util.UUID
import javax.inject.Inject

@HiltViewModel
class RulesViewModel @Inject constructor(
    private val repo: MailRepository,
    billing: Billing,
    private val notices: com.ferrowl.app.ui.notice.Notices,
) : ViewModel() {
    private val _rules = MutableStateFlow<List<Rule>>(emptyList())
    val rules: StateFlow<List<Rule>> = _rules.asStateFlow()
    val pro = billing.pro
    val folders: StateFlow<List<Folder>> = repo.folders

    init {
        reload()
    }

    private fun reload() {
        viewModelScope.launch { _rules.value = repo.rules() }
    }

    fun save(rule: Rule) {
        viewModelScope.launch {
            // Said either way, as on iOS (a rule that cannot be saved is not lost silently).
            try {
                repo.saveRule(rule)
                notices.info(com.ferrowl.core.translate("rules_saved"))
            } catch (e: com.ferrowl.core.FerrowlException) {
                notices.error(e.message.orEmpty())
            }
            reload()
        }
    }

    fun delete(rule: Rule) {
        viewModelScope.launch {
            try {
                repo.deleteRule(rule.id)
            } catch (e: com.ferrowl.core.FerrowlException) {
                notices.error(e.message.orEmpty())
            }
            reload()
        }
    }

    suspend fun preview(condition: String): List<Header> = repo.rulePreview(condition)

    fun inboxId(): String = repo.inboxId()
}

/** Settings → Rules: "when a message arrives that … then …" for the account in use (Ferrowl Pro). */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun RulesScreen(onBack: () -> Unit, vm: RulesViewModel = hiltViewModel()) {
    val rules by vm.rules.collectAsStateWithLifecycle()
    val pro by vm.pro.collectAsStateWithLifecycle()
    val folders by vm.folders.collectAsStateWithLifecycle()
    var editing by remember { mutableStateOf<Rule?>(null) }
    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(stringResource(R.string.rules_title)) },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(R.string.settings))
                    }
                },
                actions = {
                    IconButton(
                        enabled = pro,
                        onClick = {
                            editing = Rule(UUID.randomUUID().toString(), "", "", listOf(RuleAction.MarkRead), true)
                        },
                    ) { Icon(Icons.Filled.Add, contentDescription = stringResource(R.string.rules_add)) }
                },
            )
        },
    ) { padding ->
        Column(Modifier.fillMaxSize().padding(padding).verticalScroll(rememberScrollState())) {
            Text(
                stringResource(R.string.rules_desc),
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(16.dp),
            )
            if (!pro) Text(stringResource(R.string.rules_pro), modifier = Modifier.padding(horizontal = 16.dp))
            if (rules.isEmpty()) {
                Text(stringResource(R.string.rules_none), modifier = Modifier.padding(16.dp))
            }
            rules.forEach { rule ->
                ListItem(
                    headlineContent = {
                        Text(
                            rule.name.ifBlank { rule.condition },
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                            color = if (rule.enabled) MaterialTheme.colorScheme.onSurface else MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    },
                    supportingContent = { Text(summary(rule.actions, folders)) },
                    trailingContent = {
                        IconButton(onClick = { vm.delete(rule) }) {
                            Icon(Icons.Filled.Delete, contentDescription = stringResource(R.string.rules_delete))
                        }
                    },
                    modifier = Modifier.clickable { editing = rule },
                )
                HorizontalDivider()
            }
        }
    }
    editing?.let { rule ->
        RuleEditor(
            rule = rule,
            folders = folders.filter { it.selectable && it.id != vm.inboxId() },
            preview = vm::preview,
            onDismiss = { editing = null },
            onSave = {
                vm.save(it)
                editing = null
            },
        )
    }
}

@Composable
private fun summary(actions: List<RuleAction>, folders: List<Folder>): String = actions.map { a ->
    when (a) {
        RuleAction.MarkRead -> stringResource(R.string.rules_mark_read)
        RuleAction.Star -> stringResource(R.string.rules_star)
        RuleAction.Silence -> stringResource(R.string.rules_silence)
        RuleAction.Archive -> stringResource(R.string.archive)
        is RuleAction.MoveTo -> "→ " + (folders.firstOrNull { it.id == a.folder }?.name ?: a.folder)
    }
}.joinToString(" · ")

@Composable
private fun RuleEditor(
    rule: Rule,
    folders: List<Folder>,
    preview: suspend (String) -> List<Header>,
    onDismiss: () -> Unit,
    onSave: (Rule) -> Unit,
) {
    var condition by remember { mutableStateOf(rule.condition) }
    var name by remember { mutableStateOf(rule.name) }
    var enabled by remember { mutableStateOf(rule.enabled) }
    var actions by remember { mutableStateOf(rule.actions) }
    var matches by remember { mutableStateOf<List<Header>>(emptyList()) }
    LaunchedEffect(condition) {
        delay(300)
        matches = preview(condition)
    }
    fun toggle(action: RuleAction, on: Boolean) {
        actions = actions.filterNot { it == action } + if (on) listOf(action) else emptyList()
    }
    // Where it goes: nowhere (null), the Archive, or a folder.
    val destination = actions.firstOrNull { it is RuleAction.Archive || it is RuleAction.MoveTo }
    fun setDestination(action: RuleAction?) {
        actions = actions.filterNot { it is RuleAction.Archive || it is RuleAction.MoveTo } + listOfNotNull(action)
    }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.rules_title)) },
        text = {
            Column(Modifier.verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                OutlinedTextField(
                    value = condition,
                    onValueChange = { condition = it },
                    label = { Text(stringResource(R.string.rules_condition)) },
                    placeholder = { Text(stringResource(R.string.rules_condition_hint)) },
                    supportingText = { Text(stringResource(R.string.rules_condition_help)) },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth(),
                )
                Text(stringResource(R.string.rules_preview), style = MaterialTheme.typography.titleSmall)
                if (matches.isEmpty()) {
                    Text(stringResource(R.string.rules_preview_none), color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
                matches.forEach { h ->
                    Text(
                        "${h.from.ifBlank { h.fromAddress }} · ${h.subject}",
                        style = MaterialTheme.typography.bodySmall,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
                Spacer(Modifier.height(8.dp))
                Text(stringResource(R.string.rules_then), style = MaterialTheme.typography.titleSmall)
                Check(stringResource(R.string.rules_mark_read), RuleAction.MarkRead in actions) { toggle(RuleAction.MarkRead, it) }
                Check(stringResource(R.string.rules_star), RuleAction.Star in actions) { toggle(RuleAction.Star, it) }
                Check(stringResource(R.string.rules_silence), RuleAction.Silence in actions) { toggle(RuleAction.Silence, it) }
                Text(stringResource(R.string.rules_destination), style = MaterialTheme.typography.titleSmall)
                Choice(stringResource(R.string.rules_stay), destination == null) { setDestination(null) }
                Choice(stringResource(R.string.archive), destination is RuleAction.Archive) { setDestination(RuleAction.Archive) }
                folders.forEach { f ->
                    Choice(f.name, (destination as? RuleAction.MoveTo)?.folder == f.id) {
                        setDestination(RuleAction.MoveTo(f.id))
                    }
                }
                OutlinedTextField(
                    value = name,
                    onValueChange = { name = it },
                    label = { Text(stringResource(R.string.rules_name)) },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth(),
                )
                Check(stringResource(R.string.rules_enabled), enabled) { enabled = it }
            }
        },
        confirmButton = {
            Button(
                enabled = condition.isNotBlank() && actions.isNotEmpty(),
                onClick = { onSave(rule.copy(name = name.trim(), condition = condition.trim(), actions = actions, enabled = enabled)) },
            ) { Text(stringResource(R.string.rules_save)) }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.cancel)) } },
    )
}

@Composable
private fun Check(text: String, on: Boolean, onChange: (Boolean) -> Unit) {
    ListItem(
        headlineContent = { Text(text) },
        trailingContent = { Switch(checked = on, onCheckedChange = onChange) },
        modifier = Modifier.clickable { onChange(!on) },
    )
}

@Composable
private fun Choice(text: String, selected: Boolean, onClick: () -> Unit) {
    ListItem(
        headlineContent = { Text(text, maxLines = 1, overflow = TextOverflow.Ellipsis) },
        leadingContent = { RadioButton(selected = selected, onClick = onClick) },
        modifier = Modifier.fillMaxWidth().selectable(selected = selected, onClick = onClick),
    )
}
