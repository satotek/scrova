package dev.satotek.scrova.account

import android.content.Context
import dev.satotek.scrova.ScrovaApplication
import dev.satotek.scrova.scrovaApp

class AccountGraph internal constructor(app: ScrovaApplication) {
    val store: SessionStore = SessionStore(app)
    val signIn: LastfmSignIn = LastfmSignIn(app, app.scrova, store)
}

private val graphs = java.util.WeakHashMap<ScrovaApplication, AccountGraph>()

val Context.accountGraph: AccountGraph
    get() = synchronized(graphs) { graphs.getOrPut(scrovaApp) { AccountGraph(scrovaApp) } }

val Context.signIn: LastfmSignIn
    get() = accountGraph.signIn

val Context.sessionStore: SessionStore
    get() = accountGraph.store
