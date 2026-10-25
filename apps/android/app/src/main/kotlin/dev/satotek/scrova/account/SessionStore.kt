package dev.satotek.scrova.account

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import androidx.datastore.core.CorruptionException
import androidx.datastore.core.DataStoreFactory
import androidx.datastore.core.Serializer
import androidx.datastore.core.handlers.ReplaceFileCorruptionHandler
import java.io.DataInputStream
import java.io.DataOutputStream
import java.io.InputStream
import java.io.OutputStream
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.first

class LastfmSession(val sessionKey: String, val username: String) {
    override fun toString(): String = "LastfmSession(username=$username, sessionKey=<redacted>)"
}

class SessionStore(context: Context) {
    // Credentials must not be restored onto a device without its Keystore key.
    private val dataStore = DataStoreFactory.create(
        serializer = SessionSerializer(::sessionKey),
        corruptionHandler = ReplaceFileCorruptionHandler { null },
        produceFile = { context.applicationContext.noBackupFilesDir.resolve("lastfm-session.bin") },
    )

    val session: Flow<LastfmSession?> = dataStore.data

    suspend fun load(): LastfmSession? = session.first()

    suspend fun save(session: LastfmSession) {
        dataStore.updateData { session }
    }

    suspend fun clear() {
        dataStore.updateData { null }
    }

    private fun sessionKey(): SecretKey {
        val store = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (store.getKey(KEY_ALIAS, null) as? SecretKey)?.let { return it }
        return KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore").apply {
            init(
                KeyGenParameterSpec.Builder(
                    KEY_ALIAS,
                    KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
                ).setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                    .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                    .build(),
            )
        }.generateKey()
    }

    private companion object {
        const val KEY_ALIAS = "dev.satotek.scrova.lastfm.session"
    }
}

// AES-GCM supplies AEAD directly with a non-exportable Keystore key; DataStore
// owns atomic file replacement, including recovery after authentication failure.
internal class SessionSerializer(private val key: () -> SecretKey) : Serializer<LastfmSession?> {
    override val defaultValue: LastfmSession? = null

    override suspend fun readFrom(input: InputStream): LastfmSession? {
        try {
            val bytes = input.readBytes()
            if (bytes.contentEquals(byteArrayOf(0))) return null
            require(bytes.size > 13 && bytes[0] == 1.toByte())
            val cipher = Cipher.getInstance("AES/GCM/NoPadding")
            cipher.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(128, bytes.copyOfRange(1, 13)))
            cipher.updateAAD(AAD)
            return DataInputStream(cipher.doFinal(bytes.copyOfRange(13, bytes.size)).inputStream()).use {
                val session = LastfmSession(it.readUTF(), it.readUTF())
                require(it.read() == -1)
                session
            }
        } catch (error: Exception) {
            // Do not attach errors that could contain credential payloads.
            throw CorruptionException("Stored Last.fm session could not be decrypted.")
        }
    }

    override suspend fun writeTo(t: LastfmSession?, output: OutputStream) {
        if (t == null) {
            output.write(0)
            return
        }
        val plain = java.io.ByteArrayOutputStream().also { buffer ->
            DataOutputStream(buffer).use {
                it.writeUTF(t.sessionKey)
                it.writeUTF(t.username)
            }
        }.toByteArray()
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.ENCRYPT_MODE, key())
        cipher.updateAAD(AAD)
        output.write(1)
        output.write(cipher.iv)
        output.write(cipher.doFinal(plain))
    }

    private companion object {
        val AAD = "scrova.lastfm.session.v1".toByteArray(Charsets.UTF_8)
    }
}
