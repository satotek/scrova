@file:OptIn(androidx.compose.material3.ExperimentalMaterial3ExpressiveApi::class)

package dev.satotek.scrova.ui

import android.app.Activity
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.ButtonGroup
import androidx.compose.material3.ButtonGroupDefaults
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Modifier
import androidx.compose.ui.Alignment
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import dev.satotek.scrova.MainViewModel
import dev.satotek.scrova.account.SignInState

@Composable
fun SettingsScreen(model: MainViewModel, onAbout: () -> Unit) {
    val state by model.signInState.collectAsState()
    val notice by model.signInNotice.collectAsState()
    val pending by model.pending.collectAsState()
    val recent by model.recent.collectAsState()
    val access by model.notificationAccess.collectAsState()
    var confirmSignOut by rememberSaveable { mutableStateOf(false) }
    val context = LocalContext.current
    val activity = context as Activity
    LazyColumn(contentPadding = PaddingValues(20.dp), verticalArrangement = Arrangement.spacedBy(24.dp)) {
        item {
            SettingsSection("Last.fm") {
                when (val current = state) {
                    SignInState.SignedOut -> {
                        Button(onClick = { model.signIn(activity) }, enabled = model.hasLastfmCredentials) { Text("Sign in to Last.fm") }
                        if (!model.hasLastfmCredentials) {
                            Text("This build has no Last.fm API key, so it cannot sign in.", style = MaterialTheme.typography.bodyMedium)
                        }
                    }
                    is SignInState.AwaitingApproval -> {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            LoadingIndicator(Modifier.size(40.dp))
                            Text("Approve Scrova on Last.fm", modifier = Modifier.weight(1f))
                        }
                        ButtonGroup(overflowIndicator = { ButtonGroupDefaults.OverflowIndicator(it) }) {
                            clickableItem(onClick = { model.reopenApproval(activity) }, label = "Open approval page again")
                            clickableItem(onClick = model::cancelSignIn, label = "Cancel")
                        }
                    }
                    is SignInState.SignedIn -> {
                        SettingsLink(current.username, "Open Last.fm profile") { openExternal(context, lastfmProfileUrl(current.username)) }
                        TextButton(onClick = { confirmSignOut = true }, colors = ButtonDefaults.textButtonColors(contentColor = MaterialTheme.colorScheme.error)) { Text("Sign out") }
                    }
                }
            }
        }
        item {
            SettingsSection("Sending") {
                Text("Waiting to send: $pending", style = MaterialTheme.typography.bodyLarge)
                FilledTonalButton(onClick = model::sendNow, enabled = pending > 0uL && state is SignInState.SignedIn) { Text("Send now") }
                (notice ?: recent.firstNotNullOfOrNull { it.lastError })?.let {
                    Text(it, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.error)
                }
            }
        }
        item {
            SettingsSection("Permissions") {
                Text("Notification access: ${if (access) "On" else "Off"}", style = MaterialTheme.typography.bodyLarge)
                FilledTonalButton(onClick = { openNotificationSettings(context) }) { Text("Open settings") }
                if (!access) {
                    Text("If the switch is greyed out, open App info, tap ⋮ and choose Allow restricted settings.", style = MaterialTheme.typography.bodyMedium)
                    TextButton(onClick = { openAppInfo(context) }) { Text("Open App info") }
                }
            }
        }
        item { SettingsSection("About") { SettingsLink("About Scrova", onClick = onAbout) } }
    }
    if (confirmSignOut) {
        AlertDialog(
            onDismissRequest = { confirmSignOut = false },
            title = { Text("Sign out of Last.fm?") },
            text = { Text("Scrobbles that are waiting to send stay on this device.") },
            confirmButton = {
                TextButton(onClick = { confirmSignOut = false; model.signOut() }, colors = ButtonDefaults.textButtonColors(contentColor = MaterialTheme.colorScheme.error)) { Text("Sign out") }
            },
            dismissButton = { TextButton(onClick = { confirmSignOut = false }) { Text("Cancel") } },
        )
    }
}

@Composable
internal fun SettingsSection(title: String, content: @Composable ColumnScope.() -> Unit) {
    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Text(title, style = MaterialTheme.typography.titleMedium)
        Surface(shape = MaterialTheme.shapes.large, color = MaterialTheme.colorScheme.surfaceContainer) {
            Column(Modifier.fillMaxWidth().padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp), content = content)
        }
    }
}

@Composable
internal fun SettingsLink(title: String, supporting: String? = null, onClick: () -> Unit) {
    ListItem(
        onClick = onClick,
        supportingContent = supporting?.let { { Text(it) } },
        colors = ListItemDefaults.colors(containerColor = MaterialTheme.colorScheme.surfaceContainer),
    ) { Text(title) }
}
