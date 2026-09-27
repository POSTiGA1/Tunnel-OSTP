package com.ospab.ostp_client

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import java.security.KeyStore
import java.security.SecureRandom
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * The master key that seals saved SSH passwords and keys (see ostp-ssh's
 * store). It is 32 random bytes, kept in app preferences wrapped with an AES
 * key that never leaves the Android Keystore. Returns "" when the Keystore
 * does not work on this device: then nothing secret is saved.
 */
object ServerVault {
    private const val ALIAS = "ostp_servers_master"
    private const val PREFS = "OstpSecure"
    private const val ENTRY = "servers_master_key"

    @Volatile private var cached: String? = null

    @Synchronized
    fun masterKeyHex(context: Context): String {
        cached?.let { return it }
        // A key that cannot be unwrapped any more (the Keystore entry is gone,
        // e.g. after a device restore) is replaced: saved secrets are then
        // asked for again instead of breaking the servers screen.
        val hex = try {
            load(context)
        } catch (e: Throwable) {
            android.util.Log.w("ServerVault", "cannot unwrap the master key: ${e.message}")
            null
        } ?: try {
            create(context)
        } catch (e: Throwable) {
            android.util.Log.w("ServerVault", "Keystore unavailable: ${e.message}")
            ""
        }
        cached = hex
        return hex
    }

    private fun wrappingKey(): SecretKey {
        val ks = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (ks.getKey(ALIAS, null) as? SecretKey)?.let { return it }
        val gen = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
        gen.init(
            KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .build()
        )
        return gen.generateKey()
    }

    private fun load(context: Context): String? {
        val stored = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getString(ENTRY, null) ?: return null
        val raw = Base64.decode(stored, Base64.NO_WRAP)
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.DECRYPT_MODE, wrappingKey(), GCMParameterSpec(128, raw, 0, 12))
        val key = cipher.doFinal(raw, 12, raw.size - 12)
        return key.joinToString("") { "%02x".format(it) }
    }

    private fun create(context: Context): String {
        val key = ByteArray(32).also { SecureRandom().nextBytes(it) }
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.ENCRYPT_MODE, wrappingKey())
        val sealed = cipher.iv + cipher.doFinal(key)
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit()
            .putString(ENTRY, Base64.encodeToString(sealed, Base64.NO_WRAP)).commit()
        return key.joinToString("") { "%02x".format(it) }
    }
}
