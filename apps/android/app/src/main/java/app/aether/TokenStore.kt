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

/**
 * Server address and device token. The token is encrypted with an AES key that
 * never leaves the Android Keystore; backups are disabled in the manifest.
 */
class TokenStore(context: Context) {
    private val prefs = context.getSharedPreferences("aether", Context.MODE_PRIVATE)

    var server: String?
        get() = prefs.getString("server", null)
        set(value) = prefs.edit().putString("server", value).apply()

    fun load(): Token? {
        val sealed = prefs.getString("token", null) ?: return null
        return runCatching {
            val bytes = Base64.decode(sealed, Base64.NO_WRAP)
            val cipher = Cipher.getInstance(TRANSFORMATION)
            cipher.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(128, bytes, 0, IV_BYTES))
            val (access, expiresAt) = String(cipher.doFinal(bytes, IV_BYTES, bytes.size - IV_BYTES)).split('\n')
            Token(access, expiresAt.toLong())
        }.getOrNull()
    }

    fun save(token: Token?) {
        if (token == null) {
            prefs.edit().remove("token").apply()
            return
        }
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(Cipher.ENCRYPT_MODE, key())
        val sealed = cipher.iv + cipher.doFinal("${token.access}\n${token.expiresAt}".toByteArray())
        prefs.edit().putString("token", Base64.encodeToString(sealed, Base64.NO_WRAP)).apply()
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
    }
}
