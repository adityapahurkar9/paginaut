package app.paginaut.reader

import android.graphics.Typeface
import android.os.Bundle
import android.util.TypedValue
import android.view.ViewGroup.LayoutParams.MATCH_PARENT
import android.view.ViewGroup.LayoutParams.WRAP_CONTENT
import android.widget.Button
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import androidx.appcompat.app.AppCompatActivity
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat

/**
 * What a copy of Paginaut asking for permissions it doesn't use shows instead of itself (MainActivity
 * sends it here): which permissions, and what to do. A plain screen, no page and no engine: nothing
 * on the phone is opened.
 */
class RefusedActivity : AppCompatActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    super.onCreate(savedInstanceState)
    val extra = intent.getStringArrayListExtra(EXTRA) ?: arrayListOf()
    val names = extra.joinToString("\n") { p ->
      val label = try {
        packageManager.getPermissionInfo(p, 0).loadLabel(packageManager).toString()
      } catch (_: Exception) {
        null
      }
      "• " + (label?.takeIf { it != p }?.let { "$it ($p)" } ?: p)
    }
    val dp = { v: Float -> TypedValue.applyDimension(TypedValue.COMPLEX_UNIT_DIP, v, resources.displayMetrics).toInt() }
    val column = LinearLayout(this).apply {
      orientation = LinearLayout.VERTICAL
      setPadding(dp(24f), dp(24f), dp(24f), dp(24f))
      addView(TextView(this@RefusedActivity).apply {
        text = "Paginaut stopped"
        setTextSize(TypedValue.COMPLEX_UNIT_SP, 24f)
        setTypeface(typeface, Typeface.BOLD)
      })
      addView(TextView(this@RefusedActivity).apply {
        text = "This copy of Paginaut asks Android for permissions Paginaut never uses:\n\n$names\n\n" +
          "Someone may have changed it. Paginaut won't run like this, so nothing on your phone is opened.\n\n" +
          "Uninstall this copy and install Paginaut from Google Play or Paginaut's own page."
        setTextSize(TypedValue.COMPLEX_UNIT_SP, 16f)
        setLineSpacing(0f, 1.25f)
        setPadding(0, dp(16f), 0, dp(24f))
      })
      addView(Button(this@RefusedActivity).apply {
        text = "Close"
        setOnClickListener { finishAndRemoveTask() }
      }, LinearLayout.LayoutParams(WRAP_CONTENT, WRAP_CONTENT))
    }
    val root = ScrollView(this).apply {
      addView(column, MATCH_PARENT, WRAP_CONTENT)
    }
    // Clear of the status and navigation bars and any camera cutout (the app draws under them).
    ViewCompat.setOnApplyWindowInsetsListener(root) { v, insets ->
      val bars = insets.getInsets(WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout())
      v.setPadding(bars.left, bars.top, bars.right, bars.bottom)
      insets
    }
    setContentView(root)
    // Dark status bar icons on the light page (light ones in the dark theme).
    val night = resources.configuration.uiMode and android.content.res.Configuration.UI_MODE_NIGHT_MASK == android.content.res.Configuration.UI_MODE_NIGHT_YES
    WindowCompat.getInsetsController(window, window.decorView).apply {
      isAppearanceLightStatusBars = !night
      isAppearanceLightNavigationBars = !night
    }
  }

  companion object {
    const val EXTRA = "permissions"
  }
}
