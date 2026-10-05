package app.paginaut.reader

import android.app.Activity
import android.content.ActivityNotFoundException
import android.content.ClipData
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.os.CancellationSignal
import android.os.ParcelFileDescriptor
import android.print.PageRange
import android.print.PrintAttributes
import android.print.PrintDocumentAdapter
import android.print.PrintDocumentInfo
import android.print.PrintManager
import android.provider.OpenableColumns
import android.speech.tts.TextToSpeech
import android.speech.tts.UtteranceProgressListener
import android.webkit.MimeTypeMap
import android.webkit.WebView
import androidx.activity.result.ActivityResult
import androidx.core.content.FileProvider
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.io.File
import java.io.FileInputStream
import java.io.FileOutputStream
import java.security.MessageDigest
import java.util.Locale
import java.util.UUID
import java.util.concurrent.Executors

/**
 * Android's side of the viewer: picking files, files sent by other apps ("Open with", Share),
 * sharing and printing. The engine reads files by path, so documents are copied into the
 * app's cache (named by their source, so a file opened again reuses its copy when unchanged).
 */
@TauriPlugin
class PaginautPlugin(private val activity: Activity) : Plugin(activity) {
  private val io = Executors.newSingleThreadExecutor()

  /** A file another app sent; taken by takeIntent. */
  @Volatile private var pending: Uri? = null
  private var webView: WebView? = null

  override fun load(webView: WebView) {
    this.webView = webView
    pending = docUri(activity.intent)
    // Android 15 and later always draw the app behind the status and navigation bars. The page keeps
    // clear of them, the camera cutout and the keyboard itself, from the sizes passed here as CSS
    // pixels. (Padding the WebView's parent didn't last: AppCompat resets it on rotation.)
    ViewCompat.setOnApplyWindowInsetsListener(webView) { v, insets ->
      val bars = insets.getInsets(WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout())
      val ime = insets.getInsets(WindowInsetsCompat.Type.ime())
      val d = v.resources.displayMetrics.density
      safeArea = floatArrayOf(bars.top / d, bars.right / d, maxOf(bars.bottom, ime.bottom) / d, bars.left / d)
      sendSafeArea()
      insets
    }
    ViewCompat.requestApplyInsets(webView)
  }

  /** Top, right, bottom, left: what the page must keep clear of, in CSS pixels. */
  @Volatile private var safeArea = floatArrayOf(0f, 0f, 0f, 0f)

  private fun sendSafeArea() {
    val (t, r, b, l) = safeArea.toList()
    activity.runOnUiThread { webView?.evaluateJavascript("window.paginautSafeArea && window.paginautSafeArea($t, $r, $b, $l)", null) }
  }

  /** The current safe area, for the page as it starts (sizes sent before it was listening are lost). */
  @Command
  fun safeArea(invoke: Invoke) {
    val (t, r, b, l) = safeArea.toList()
    invoke.resolve(JSObject().put("top", t.toDouble()).put("right", r.toDouble()).put("bottom", b.toDouble()).put("left", l.toDouble()))
  }

  override fun onNewIntent(intent: Intent) {
    val uri = docUri(intent) ?: return
    pending = uri
    // Tell the page (it may have asked already, as the app came to the front, and found nothing).
    activity.runOnUiThread { webView?.evaluateJavascript("window.paginautIntent && window.paginautIntent()", null) }
  }

  private fun docUri(intent: Intent?): Uri? = when (intent?.action) {
    Intent.ACTION_VIEW -> intent.data
    Intent.ACTION_SEND -> @Suppress("DEPRECATION") (intent.getParcelableExtra(Intent.EXTRA_STREAM) as? Uri)
    else -> null
  }

  // ------------------------------------------------------------ opening

  @Command
  fun pickFile(invoke: Invoke) {
    val intent = Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
      addCategory(Intent.CATEGORY_OPENABLE)
      type = "*/*"
      putExtra(Intent.EXTRA_MIME_TYPES, MIME_TYPES)
    }
    try {
      startActivityForResult(invoke, intent, "picked")
    } catch (e: ActivityNotFoundException) {
      invoke.reject("No file picker is available on this device.")
    }
  }

  @ActivityCallback
  fun picked(invoke: Invoke, result: ActivityResult) {
    val uri = result.data?.data
    if (result.resultCode != Activity.RESULT_OK || uri == null) {
      invoke.resolve(JSObject())
      return
    }
    // Keep access, so the file can be opened again from Recent after a restart.
    try {
      activity.contentResolver.takePersistableUriPermission(uri, Intent.FLAG_GRANT_READ_URI_PERMISSION)
    } catch (_: SecurityException) {
    }
    copyThenResolve(invoke, uri)
  }

  /** The file another app sent ("Open with", Share), once; empty when there's none. */
  @Command
  fun takeIntent(invoke: Invoke) {
    val uri = pending
    pending = null
    if (uri == null) invoke.resolve(JSObject()) else copyThenResolve(invoke, uri)
  }

  /** Opens a recent file again from where it came from (its copy is refreshed if it changed). */
  @Command
  fun reopen(invoke: Invoke) {
    val uri = invoke.getArgs().getString("uri", null)
    if (uri.isNullOrEmpty()) return invoke.reject("No file given.")
    copyThenResolve(invoke, Uri.parse(uri))
  }

  private fun copyThenResolve(invoke: Invoke, uri: Uri) {
    io.execute {
      try {
        val (file, name) = copyToCache(uri)
        invoke.resolve(JSObject().apply {
          put("path", file.absolutePath)
          put("name", name)
          put("uri", uri.toString())
        })
      } catch (e: SecurityException) {
        invoke.reject("Paginaut can no longer open this file. Open it again from its app or Files.")
      } catch (e: Exception) {
        invoke.reject("Couldn't read the file: ${e.message ?: e.javaClass.simpleName}")
      }
    }
  }

  private fun copyToCache(uri: Uri): Pair<File, String> {
    val cr = activity.contentResolver
    var name: String? = null
    var size = -1L
    if (uri.scheme == "file") {
      val f = File(uri.path ?: "")
      name = f.name
      size = f.length()
    } else {
      cr.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE), null, null, null)?.use { c ->
        if (c.moveToFirst()) {
          val ni = c.getColumnIndex(OpenableColumns.DISPLAY_NAME)
          val si = c.getColumnIndex(OpenableColumns.SIZE)
          if (ni >= 0 && !c.isNull(ni)) name = c.getString(ni)
          if (si >= 0 && !c.isNull(si)) size = c.getLong(si)
        }
      }
    }
    // The engine tells formats apart by extension: give the copy one.
    var display = (name ?: uri.lastPathSegment?.substringAfterLast('/') ?: "document").replace('/', '_')
    if (!display.contains('.')) {
      val ext = cr.getType(uri)?.let { MimeTypeMap.getSingleton().getExtensionFromMimeType(it) }
      if (ext != null) display = "$display.$ext"
    }
    val dir = File(activity.cacheDir, "docs/" + sha1(uri.toString()).take(16))
    val out = File(dir, display)
    if (size >= 0 && out.isFile && out.length() == size) return Pair(out, display)
    // Read the source before dropping the old copy: if it can't be read any more (a file another
    // app lent us for one visit), the copy is what Recent opens instead.
    val input = cr.openInputStream(uri) ?: throw IllegalStateException("the file is empty")
    dir.mkdirs()
    val tmp = File(dir, ".part")
    input.use { i -> FileOutputStream(tmp).use { o -> i.copyTo(o, 1 shl 16) } }
    dir.listFiles()?.forEach { if (it != tmp) it.delete() }
    if (!tmp.renameTo(out)) throw IllegalStateException("couldn't store a copy")
    trimCache(dir)
    return Pair(out, display)
  }

  /** Keeps the copies under ~1 GB, dropping the least recently opened (never the one just made). */
  private fun trimCache(keep: File) {
    val dirs = File(activity.cacheDir, "docs").listFiles()?.filter { it.isDirectory && it != keep } ?: return
    var total = dirs.sumOf { d -> d.listFiles()?.sumOf { it.length() } ?: 0L }
    for (d in dirs.sortedBy { it.lastModified() }) {
      if (total < CACHE_LIMIT) break
      total -= d.listFiles()?.sumOf { it.length() } ?: 0L
      d.deleteRecursively()
    }
  }

  // ------------------------------------------------------------ sharing and printing

  @Command
  fun share(invoke: Invoke) {
    val args = invoke.getArgs()
    val path = args.getString("path", null) ?: return invoke.reject("No file given.")
    val file = File(path)
    val uri = try {
      FileProvider.getUriForFile(activity, activity.packageName + ".fileprovider", file)
    } catch (e: IllegalArgumentException) {
      return invoke.reject("This file can't be shared.")
    }
    val mime = mimeOf(file.name)
    val send = Intent(Intent.ACTION_SEND).apply {
      type = mime
      putExtra(Intent.EXTRA_STREAM, uri)
      clipData = ClipData.newRawUri(file.name, uri)
      addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
    }
    val chooser = Intent.createChooser(send, null)
    // Sharing a file to Paginaut from Paginaut would only open it again.
    chooser.putExtra(Intent.EXTRA_EXCLUDE_COMPONENTS, arrayOf(android.content.ComponentName(activity, MainActivity::class.java)))
    activity.startActivity(chooser)
    invoke.resolve()
  }

  /** Hands a PDF to Android's print service (its own dialog: printer, pages, copies, Save as PDF). */
  @Command
  fun print(invoke: Invoke) {
    val args = invoke.getArgs()
    val path = args.getString("path", null) ?: return invoke.reject("No file given.")
    val title = args.getString("title", null) ?: File(path).nameWithoutExtension
    val pm = activity.getSystemService(Context.PRINT_SERVICE) as? PrintManager
      ?: return invoke.reject("Printing isn't available on this device.")
    activity.runOnUiThread {
      pm.print(title, FileAdapter(File(path), title), PrintAttributes.Builder().build())
      invoke.resolve()
    }
  }

  private class FileAdapter(private val file: File, private val title: String) : PrintDocumentAdapter() {
    override fun onLayout(old: PrintAttributes?, new: PrintAttributes?, cancel: CancellationSignal?, cb: LayoutResultCallback, extras: Bundle?) {
      if (cancel?.isCanceled == true) return cb.onLayoutCancelled()
      val info = PrintDocumentInfo.Builder("$title.pdf").setContentType(PrintDocumentInfo.CONTENT_TYPE_DOCUMENT).build()
      cb.onLayoutFinished(info, old != new)
    }

    override fun onWrite(pages: Array<out PageRange>?, dest: ParcelFileDescriptor, cancel: CancellationSignal?, cb: WriteResultCallback) {
      try {
        FileInputStream(file).use { i -> FileOutputStream(dest.fileDescriptor).use { o -> i.copyTo(o, 1 shl 16) } }
        if (cancel?.isCanceled == true) cb.onWriteCancelled() else cb.onWriteFinished(arrayOf(PageRange.ALL_PAGES))
      } catch (e: Exception) {
        cb.onWriteFailed(e.message)
      }
    }
  }

  // ------------------------------------------------------------ read aloud

  private var tts: TextToSpeech? = null
  private var ttsState = 0 // 0 not started, 1 starting, 2 ready, -1 unavailable
  private val ttsWaiting = mutableListOf<() -> Unit>()
  /** Utterances queued, by id: resolved when spoken (or stopped). */
  private val speaking = java.util.concurrent.ConcurrentHashMap<String, Invoke>()

  private fun withTts(then: (Boolean) -> Unit) {
    when (ttsState) {
      2 -> return then(true)
      -1 -> return then(false)
    }
    ttsWaiting.add { then(ttsState == 2) }
    if (ttsState == 1) return
    ttsState = 1
    tts = TextToSpeech(activity) { status ->
      activity.runOnUiThread {
        ttsState = if (status == TextToSpeech.SUCCESS) 2 else -1
        tts?.setOnUtteranceProgressListener(object : UtteranceProgressListener() {
          override fun onStart(id: String) {}
          override fun onDone(id: String) {
            speaking.remove(id)?.resolve(JSObject().put("stopped", false))
          }
          @Deprecated("Deprecated in Java")
          override fun onError(id: String) {
            speaking.remove(id)?.reject("This text couldn't be read aloud.")
          }
          override fun onError(id: String, code: Int) {
            val msg = when (code) {
              TextToSpeech.ERROR_NOT_INSTALLED_YET -> "The voice for this language isn't downloaded yet. Add it in Android's settings (Text-to-speech), or try again in a moment."
              TextToSpeech.ERROR_NETWORK, TextToSpeech.ERROR_NETWORK_TIMEOUT -> "This voice needs the internet. Choose an offline voice in Android's settings (Text-to-speech)."
              else -> "This text couldn't be read aloud."
            }
            speaking.remove(id)?.reject(msg)
          }
          override fun onStop(id: String, interrupted: Boolean) {
            speaking.remove(id)?.resolve(JSObject().put("stopped", true))
          }
        })
        val waiting = ttsWaiting.toList()
        ttsWaiting.clear()
        waiting.forEach { it() }
      }
    }
  }

  /**
   * Queues `text` with Android's voice for its language (Devanagari as Hindi or Marathi, when the
   * phone has those voices); resolves once it has been spoken, `{ stopped: true }` if stopped first.
   */
  @Command
  fun speak(invoke: Invoke) {
    val args = invoke.getArgs()
    val text = args.getString("text", null) ?: return invoke.reject("Nothing to read.")
    val rate = args.optDouble("rate", 1.0).toFloat()
    val lang = args.getString("lang", null)
    activity.runOnUiThread {
      withTts { ok ->
        val t = tts
        if (!ok || t == null) {
          invoke.reject("Reading aloud needs a text-to-speech engine. Install or turn one on in Android's settings (Accessibility › Text-to-speech).")
          return@withTts
        }
        t.setSpeechRate(rate)
        val voice = voiceFor(t, text, lang)
        if (voice != null) t.voice = voice else t.language = Locale.getDefault()
        val id = UUID.randomUUID().toString()
        speaking[id] = invoke
        if (t.speak(text, TextToSpeech.QUEUE_ADD, null, id) != TextToSpeech.SUCCESS) {
          speaking.remove(id)
          invoke.reject("This text couldn't be read aloud.")
        }
      }
    }
  }

  /**
   * A voice installed on the phone for the text's language: Devanagari (Hindi, or Marathi when that's
   * the phone's language) when most of its letters are, otherwise the phone's language. Offline and
   * better voices first; none (the engine's default) if the phone has no voice for it.
   */
  private fun voiceFor(t: TextToSpeech, text: String, lang: String?): android.speech.tts.Voice? {
    val want = lang?.let { Locale.forLanguageTag(it).language } ?: run {
      val deva = text.count { it in 'ऀ'..'ॿ' }
      val latin = text.count { it in 'A'..'Z' || it in 'a'..'z' }
      val phone = Locale.getDefault().language
      if (deva > latin) (if (phone == "mr") "mr" else "hi") else phone
    }
    val voices = try { t.voices } catch (_: Exception) { null } ?: return null
    return voices
      .filter { it.locale.language == want && !it.features.contains(TextToSpeech.Engine.KEY_FEATURE_NOT_INSTALLED) }
      .sortedWith(compareBy<android.speech.tts.Voice>({ it.isNetworkConnectionRequired }, { -it.quality }))
      .firstOrNull()
  }

  @Command
  fun stopSpeaking(invoke: Invoke) {
    activity.runOnUiThread {
      tts?.stop()
      // Queued utterances that never started get no onStop.
      for (id in speaking.keys.toList()) speaking.remove(id)?.resolve(JSObject().put("stopped", true))
      invoke.resolve()
    }
  }

  override fun onDestroy() {
    tts?.shutdown()
    tts = null
  }

  // ------------------------------------------------------------ system bars

  /**
   * The status and navigation bars: light or dark icons, and hidden while reading (`immersive`; a
   * swipe from the edge shows them for a moment). Their background is the page's own (it draws
   * under them).
   */
  @Command
  fun setBars(invoke: Invoke) {
    val args = invoke.getArgs()
    val dark = args.optBoolean("dark", false)
    val immersive = args.optBoolean("immersive", false)
    activity.runOnUiThread {
      val window = activity.window
      val ctl = WindowCompat.getInsetsController(window, window.decorView)
      ctl.isAppearanceLightStatusBars = !dark
      ctl.isAppearanceLightNavigationBars = !dark
      if (immersive) {
        ctl.systemBarsBehavior = WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
        ctl.hide(WindowInsetsCompat.Type.systemBars())
      } else {
        ctl.show(WindowInsetsCompat.Type.systemBars())
      }
      invoke.resolve()
    }
  }

  // ------------------------------------------------------------ helpers

  private fun mimeOf(name: String): String {
    val ext = name.substringAfterLast('.', "").lowercase()
    return EXT_MIME[ext] ?: MimeTypeMap.getSingleton().getMimeTypeFromExtension(ext) ?: "application/octet-stream"
  }

  private fun sha1(s: String): String =
    MessageDigest.getInstance("SHA-1").digest(s.toByteArray()).joinToString("") { "%02x".format(it) }

  companion object {
    private const val CACHE_LIMIT = 1L shl 30

    /** What the picker offers: PDF, XPS, e-books, comics, Markdown, pictures (and unnamed files, as many comics are). */
    val MIME_TYPES = arrayOf(
      "application/pdf",
      "application/vnd.ms-xpsdocument", "application/oxps",
      "application/epub+zip", "application/x-mobipocket-ebook", "application/x-fictionbook+xml", "application/x-fictionbook",
      "application/vnd.comicbook+zip", "application/x-cbz", "application/x-cbr", "application/vnd.comicbook-rar",
      "text/markdown", "text/x-markdown", "text/plain",
      "image/*",
      "application/zip", "application/octet-stream",
    )

    private val EXT_MIME = mapOf(
      "pdf" to "application/pdf",
      "xps" to "application/vnd.ms-xpsdocument",
      "oxps" to "application/oxps",
      "epub" to "application/epub+zip",
      "mobi" to "application/x-mobipocket-ebook",
      "fb2" to "application/x-fictionbook+xml",
      "cbz" to "application/vnd.comicbook+zip",
      "md" to "text/markdown",
    )
  }
}
