package app.paginaut.reader

import android.content.Intent
import android.content.pm.ApplicationInfo
import android.content.pm.PackageManager
import android.os.Bundle
import androidx.activity.enableEdgeToEdge

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
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
