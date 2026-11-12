package app.aether

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec
import uniffi.aether_ffi.Token

/** An account signed in on this device and set aside while another one is used. */
data class SavedAccount(val id: String, val name: String, val token: Token)

/**
 * Server address and device tokens: the active account's, and those of the
 * accounts set aside. Tokens are encrypted with an AES key that never leaves
 * the Android Keystore; backups are disabled in the manifest.
 */
class TokenStore(context: Context) {
    private val prefs = context.getSharedPreferences("aether", Context.MODE_PRIVATE)

    var server: String?
        get() = prefs.getString("server", null)
        set(value) = prefs.edit().putString("server", value).apply()

    /** Who the active token belongs to, once the server said so. */
    var accountId: String?
        get() = prefs.getString("account_id", null)
        set(value) = put("account_id", value)
    var accountName: String?
        get() = prefs.getString("account_name", null)
        set(value) = put("account_name", value)

    /**
     * Verifier of a single sign-on waiting in the browser. Kept on disk because
     * the application may be stopped while the browser is in front; it is
     * useless without the one-time code only this application receives.
     */
    var ssoVerifier: String?
        get() = prefs.getString("sso_verifier", null)
        set(value) = put("sso_verifier", value)

    fun load(): Token? = open("token")?.let(::token)

    fun save(token: Token?) = seal("token", token?.let { "${it.access}\n${it.expiresAt}" })

    /** The accounts set aside on this server, most recently used first. */
    fun parked(): List<SavedAccount> = open("accounts").orEmpty().split(RECORD).mapNotNull { record ->
        val fields = record.split(FIELD)
        if (fields.size != 4) return@mapNotNull null
        val expiresAt = fields[3].toLongOrNull() ?: return@mapNotNull null
        SavedAccount(fields[0], fields[1], Token(fields[2], expiresAt))
    }

    fun setParked(accounts: List<SavedAccount>) = seal(
        "accounts",
        accounts.takeIf { it.isNotEmpty() }?.joinToString(RECORD) {
            listOf(it.id, it.name.filter { c -> !c.isISOControl() }, it.token.access, it.token.expiresAt).joinToString(FIELD)
        },
    )

    private fun put(name: String, value: String?) =
        prefs.edit().apply { if (value == null) remove(name) else putString(name, value) }.apply()

    private fun token(text: String): Token? {
        val (access, expiresAt) = text.split('\n').takeIf { it.size == 2 } ?: return null
        return Token(access, expiresAt.toLongOrNull() ?: return null)
    }

    private fun open(name: String): String? {
        val sealed = prefs.getString(name, null) ?: return null
        return runCatching {
            val bytes = Base64.decode(sealed, Base64.NO_WRAP)
            val cipher = Cipher.getInstance(TRANSFORMATION)
            cipher.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(128, bytes, 0, IV_BYTES))
            String(cipher.doFinal(bytes, IV_BYTES, bytes.size - IV_BYTES))
        }.getOrNull()
    }

    private fun seal(name: String, text: String?) {
        if (text == null) {
            prefs.edit().remove(name).apply()
            return
        }
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(Cipher.ENCRYPT_MODE, key())
        val sealed = cipher.iv + cipher.doFinal(text.toByteArray())
        prefs.edit().putString(name, Base64.encodeToString(sealed, Base64.NO_WRAP)).apply()
    }

    private fun key(): SecretKey {
        val keyStore = KeyStore.getInstance(KEYSTORE).apply { load(null) }
        (keyStore.getKey(ALIAS, null) as? SecretKey)?.let { return it }
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, KEYSTORE)
        generator.init(
            KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .build(),
        )
        return generator.generateKey()
    }

    private companion object {
        const val KEYSTORE = "AndroidKeyStore"
        const val ALIAS = "aether-device-token"
        const val TRANSFORMATION = "AES/GCM/NoPadding"
        const val IV_BYTES = 12
        const val FIELD = "\u001f"
        const val RECORD = "\u001e"
    }
}
