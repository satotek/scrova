@file:OptIn(androidx.compose.material3.ExperimentalMaterial3ExpressiveApi::class, androidx.compose.material3.ExperimentalMaterial3Api::class)

package dev.satotek.scrova.ui

import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.ShortNavigationBar
import androidx.compose.material3.ShortNavigationBarItem
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.painterResource
import androidx.navigation.NavGraph.Companion.findStartDestination
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.currentBackStackEntryAsState
import androidx.navigation.compose.rememberNavController
import dev.satotek.scrova.MainViewModel
import dev.satotek.scrova.R

private data class Destination(val route: String, val title: String, val icon: Int)
private val tabs = listOf(
    Destination("now", "Now playing", R.drawable.ic_music_note),
    Destination("history", "History", R.drawable.ic_history),
    Destination("settings", "Settings", R.drawable.ic_settings),
)

@Composable
fun ScrovaApp(model: MainViewModel) {
    val nav = rememberNavController()
    val entry by nav.currentBackStackEntryAsState()
    val route = entry?.destination?.route ?: "now"
    val title = tabs.find { it.route == route }?.title ?: when (route) {
        "about" -> "About Scrova"
        "privacy" -> "Privacy policy"
        else -> "Open-source licenses"
    }
    fun selectTab(target: String) {
        nav.navigate(target) {
            popUpTo(nav.graph.findStartDestination().id) { saveState = true }
            launchSingleTop = true
            restoreState = true
        }
    }
    val motion = MaterialTheme.motionScheme
    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(title) },
                navigationIcon = {
                    if (tabs.none { it.route == route }) {
                        IconButton(onClick = { nav.popBackStack() }) {
                            Icon(Icons.AutoMirrored.Default.ArrowBack, contentDescription = "Back")
                        }
                    }
                },
            )
        },
        bottomBar = {
            if (tabs.any { it.route == route }) {
                ShortNavigationBar {
                    tabs.forEach { tab ->
                        ShortNavigationBarItem(
                            selected = route == tab.route,
                            onClick = { selectTab(tab.route) },
                            icon = { Icon(painterResource(tab.icon), contentDescription = null) },
                            label = { Text(tab.title) },
                        )
                    }
                }
            }
        },
    ) { padding ->
        NavHost(
            navController = nav,
            startDestination = "now",
            modifier = Modifier.padding(padding),
            enterTransition = { fadeIn(motion.defaultEffectsSpec()) },
            exitTransition = { fadeOut(motion.fastEffectsSpec()) },
        ) {
            composable("now") { HomeScreen(model, onHistory = { selectTab("history") }, onSettings = { selectTab("settings") }) }
            composable("history") { HistoryScreen(model) }
            composable("settings") { SettingsScreen(model, onAbout = { nav.navigate("about") }) }
            composable("about") { AboutScreen(onPrivacy = { nav.navigate("privacy") }, onLicenses = { nav.navigate("licenses") }) }
            composable("privacy") { PrivacyScreen() }
            composable("licenses") { LicensesScreen() }
        }
    }
}
