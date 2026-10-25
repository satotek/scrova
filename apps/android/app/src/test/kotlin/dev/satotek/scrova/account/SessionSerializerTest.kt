package dev.satotek.scrova.account

import androidx.datastore.core.CorruptionException
import java.io.ByteArrayOutputStream
import javax.crypto.KeyGenerator
import kotlinx.coroutines.runBlocking
import org.junit.Assert.*
import org.junit.Test

class SessionSerializerTest {
    private val key = KeyGenerator.getInstance("AES").apply { init(256) }.generateKey()
    private val serializer = SessionSerializer { key }

    @Test fun roundTrip() = runBlocking {
        val original = LastfmSession("test-session", "listener")
        val output = ByteArrayOutputStream()
        serializer.writeTo(original, output)
        assertFalse(output.toString(Charsets.UTF_8.name()).contains(original.sessionKey))
        val restored = serializer.readFrom(output.toByteArray().inputStream())!!
        assertEquals(original.sessionKey, restored.sessionKey)
        assertEquals(original.username, restored.username)
        assertFalse(original.toString().contains(original.sessionKey))
    }

    @Test fun clearRoundTrip() = runBlocking {
        val output = ByteArrayOutputStream()
        serializer.writeTo(null, output)
        assertNull(serializer.readFrom(output.toByteArray().inputStream()))
    }

    @Test fun tamperingIsCorruption() = runBlocking {
        val output = ByteArrayOutputStream()
        serializer.writeTo(LastfmSession("test-session", "listener"), output)
        val bytes = output.toByteArray().apply { this[lastIndex] = (last().toInt() xor 1).toByte() }
        try {
            serializer.readFrom(bytes.inputStream())
            fail("Tampered ciphertext must not load")
        } catch (_: CorruptionException) {
        }
    }

    @Test fun lostKeyIsCorruption() = runBlocking {
        val output = ByteArrayOutputStream()
        serializer.writeTo(LastfmSession("test-session", "listener"), output)
        val otherKey = KeyGenerator.getInstance("AES").apply { init(256) }.generateKey()
        try {
            SessionSerializer { otherKey }.readFrom(output.toByteArray().inputStream())
            fail("A replaced key must not decrypt the session")
        } catch (_: CorruptionException) {
        }
    }
}
