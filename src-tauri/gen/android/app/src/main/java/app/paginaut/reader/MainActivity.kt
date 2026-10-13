package app.paginaut.reader

import android.app.Activity
import android.content.Context
import android.content.Intent
import android.os.Build
import android.view.WindowManager
import android.content.pm.ApplicationInfo
import android.content.pm.PackageManager
import android.os.Bundle
import androidx.activity.enableEdgeToEdge

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    applySecure(this, getSharedPreferences(PREFS, Context.MODE_PRIVATE).getBoolean("secure", false))
    // Checked before the page starts: a refused copy reads no file (PaginautPlugin checks `refused`)
    // and shows RefusedActivity in its place.
    val extra = extraPermissions()
    refused = extra.isNotEmpty()
    super.onCreate(savedInstanceState)
    if (refused) {
      startActivity(Intent(this, RefusedActivity::class.java).putStringArrayListExtra(RefusedActivity.EXTRA, ArrayList(extra)))
      finish()
    }
  }

  companion object {
    /** This copy asks for permissions Paginaut doesn't use: it opens and lists nothing. */
    @Volatile
    var refused = false

    private const val PREFS = "paginaut"

    /** Hide from screenshots, for the next start (the page's setting is the one shown). */
    fun rememberSecure(context: Context, on: Boolean) {
      context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().putBoolean("secure", on).apply()
    }

    /** Android's secure window on or off (on the UI thread, or before the window shows). */
    fun applySecure(activity: Activity, on: Boolean) {
      if (on) activity.window.addFlags(WindowManager.LayoutParams.FLAG_SECURE)
      else activity.window.clearFlags(WindowManager.LayoutParams.FLAG_SECURE)
      // Android 13+: no preview in Recent apps either way the flag is read.
      if (Build.VERSION.SDK_INT >= 33) activity.setRecentsScreenshotEnabled(!on)
    }
  }

  /**
   * Permissions this copy of Paginaut asks Android for beyond the ones it uses: reading storage
   * (only once you turn on all-files access) and one AndroidX adds for its own private broadcasts.
   * Debug builds also have the internet, to load the page from the computer building them. Anything
   * else means someone changed the app (or a library slipped one in: app/build.gradle.kts stops
   * such a release from being built).
   */
  private fun extraPermissions(): List<String> {
    val debug = applicationInfo.flags and ApplicationInfo.FLAG_DEBUGGABLE != 0
    val allowed = setOf(
      "android.permission.MANAGE_EXTERNAL_STORAGE",
      "android.permission.READ_EXTERNAL_STORAGE",
      "$packageName.DYNAMIC_RECEIVER_NOT_EXPORTED_PERMISSION",
    ) + if (debug) setOf("android.permission.INTERNET") else emptySet()
    val asked = try {
      @Suppress("DEPRECATION")
      packageManager.getPackageInfo(packageName, PackageManager.GET_PERMISSIONS).requestedPermissions?.toList() ?: emptyList()
    } catch (_: Exception) {
      emptyList()
    }
    return asked.filter { it !in allowed }
  }
}
