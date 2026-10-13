package app.paginaut.reader

import android.Manifest
import android.app.Activity
import android.content.ActivityNotFoundException
import android.content.ClipData
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.ServiceConnection
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.CancellationSignal
import android.os.Environment
import android.os.IBinder
import android.os.Message
import android.os.Messenger
import android.os.ParcelFileDescriptor
import android.print.PageRange
import android.print.PrintAttributes
import android.print.PrintDocumentAdapter
import android.print.PrintDocumentInfo
import android.print.PrintManager
import android.provider.DocumentsContract
import android.provider.OpenableColumns
import android.system.ErrnoException
import android.system.Os
import android.system.OsConstants
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
import java.io.FileDescriptor
import java.io.FileInputStream
import java.io.FileOutputStream
import java.io.RandomAccessFile
import java.security.MessageDigest
import java.util.Locale
import java.util.UUID
import java.util.concurrent.Executors
import org.json.JSONArray

/**
 * Android's side of the viewer: picking files, files sent by other apps ("Open with", Share),
 * sharing and printing. The engine reads files by path: files Paginaut can reach again (picked
 * here, in a folder chosen for the shelf) are read where they are; files another app lends for one
 * visit are copied into the app's cache (named by their source, so a file opened again reuses its
 * copy when unchanged), and the page decides which copies to keep (records.ts).
 */
@TauriPlugin
class PaginautPlugin(private val activity: Activity) : Plugin(activity) {
  private val io = Executors.newSingleThreadExecutor()

  /** A file another app sent; taken by takeIntent. */
  @Volatile private var pending: Uri? = null
  private var webView: WebView? = null

  override fun load(webView: WebView) {
    this.webView = webView
    // Big files read in place last time: their links (and any one-visit copies) go.
    File(activity.cacheDir, "direct").deleteRecursively()
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
    if (MainActivity.refused) return invoke.reject(REFUSED)
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
    if (MainActivity.refused) return invoke.reject(REFUSED)
    val uri = pending
    pending = null
    if (uri == null) invoke.resolve(JSObject()) else copyThenResolve(invoke, uri)
  }

  /** Opens a recent file again from where it came from (its copy is refreshed if it changed). */
  @Command
  fun reopen(invoke: Invoke) {
    if (MainActivity.refused) return invoke.reject(REFUSED)
    val uri = invoke.getArgs().getString("uri", null)
    if (uri.isNullOrEmpty()) return invoke.reject("No file given.")
    copyThenResolve(invoke, Uri.parse(uri))
  }

  private fun copyThenResolve(invoke: Invoke, uri: Uri) {
    io.execute {
      try {
        invoke.resolve(open(uri).json(uri))
      } catch (e: SecurityException) {
        // Lent by another app for one visit: the copy made then is what there is (counted as opened).
        val copy = cachedCopy(uri)
        if (copy != null) {
          copy.parentFile?.setLastModified(System.currentTimeMillis())
          invoke.resolve(Opened(copy, copy.name, inPlace = false, lasting = false, kept = true, fp = fingerprint(copy)).json(uri))
        } else invoke.reject("Paginaut can no longer open this file. Open it again from its app or Files.")
      } catch (e: java.io.FileNotFoundException) {
        invoke.reject("This file isn't there any more (moved or deleted).")
      } catch (e: Exception) {
        invoke.reject("Couldn't read the file: ${e.message ?: e.javaClass.simpleName}")
      }
    }
  }

  /** Files read in place (path of the link to the file), or copied for one visit (null). */
  private val direct = HashMap<String, ParcelFileDescriptor?>()

  /**
   * A file read where it is: Android's handle to it stays open, and a link named like the file
   * (MuPDF tells formats by extension) points at it through /proc/self/fd. Null when the source
   * can't be read at random (a stream from a cloud app, say).
   */
  private fun openDirect(uri: Uri, name: String, dir: File): Pair<File, ParcelFileDescriptor>? {
    val pfd = activity.contentResolver.openFileDescriptor(uri, "r") ?: return null
    try {
      Os.lseek(pfd.fileDescriptor, 0, OsConstants.SEEK_SET)
    } catch (e: ErrnoException) {
      pfd.close()
      return null
    }
    val link = File(dir, name)
    try {
      Os.symlink("/proc/self/fd/${pfd.fd}", link.absolutePath)
    } catch (e: ErrnoException) {
      pfd.close()
      return null
    }
    synchronized(direct) { direct[link.absolutePath] = pfd }
    return link to pfd
  }

  /** Lets go of a file read in place, or deletes its one-visit copy (the document has closed). */
  @Command
  fun release(invoke: Invoke) {
    val path = invoke.getArgs().getString("path", null) ?: return invoke.resolve()
    val known = synchronized(direct) { direct.containsKey(path) }
    if (known) {
      synchronized(direct) { direct.remove(path) }?.close()
      File(path).parentFile?.deleteRecursively()
    }
    invoke.resolve()
  }

  /** The copy kept for a source, if there is one. */
  private fun cachedCopy(uri: Uri): File? = copyIn(File(activity.cacheDir, "docs/" + sha1(uri.toString()).take(16)))

  /** The copy in a cache folder (not a half-made one, nor a PDF made for printing). */
  private fun copyIn(dir: File): File? =
    dir.listFiles()?.firstOrNull { it.isFile && it.name != ".part" && !it.name.endsWith(".print.pdf") }

  /**
   * A document for MuPDF: its file and name; whether it is read in place (let go when it closes),
   * can be opened again from where it is (picked here, in a chosen folder, or a path with all-files
   * access), or is a copy kept in the cache; and a fingerprint of its contents (to know it again
   * whichever app sends it).
   */
  private data class Opened(val file: File, val name: String, val inPlace: Boolean, val lasting: Boolean, val kept: Boolean, val fp: String) {
    fun json(uri: Uri) = JSObject().apply {
      put("path", file.absolutePath)
      put("name", name)
      put("uri", uri.toString())
      put("inPlace", inPlace)
      put("lasting", lasting)
      put("kept", kept)
      put("fp", fp)
    }
  }

  /** The file's name as its source tells it (with an extension: the engine tells formats by it), and its size (-1: unknown). */
  private fun nameAndSize(uri: Uri): Pair<String, Long> {
    val cr = activity.contentResolver
    var name: String? = null
    var size = -1L
    cr.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE), null, null, null)?.use { c ->
      if (c.moveToFirst()) {
        val ni = c.getColumnIndex(OpenableColumns.DISPLAY_NAME)
        val si = c.getColumnIndex(OpenableColumns.SIZE)
        if (ni >= 0 && !c.isNull(ni)) name = c.getString(ni)
        if (si >= 0 && !c.isNull(si)) size = c.getLong(si)
      }
    }
    var display = (name ?: uri.lastPathSegment?.substringAfterLast('/') ?: "document").replace('/', '_')
    if (!display.contains('.')) {
      val ext = cr.getType(uri)?.let { MimeTypeMap.getSingleton().getExtensionFromMimeType(it) }
      if (ext != null) display = "$display.$ext"
    }
    return display to size
  }

  private fun open(uri: Uri): Opened {
    val cr = activity.contentResolver
    val lasting = canReach(uri)
    if (uri.scheme == "file") {
      // Only paths the shelf found with all-files access (other apps can't send these).
      if (!lasting) throw SecurityException("no access")
      val f = File(uri.path ?: "")
      if (!f.isFile) throw java.io.FileNotFoundException(f.path)
      return Opened(f, f.name, inPlace = false, lasting = true, kept = false, fp = fingerprint(f))
    }
    val (display, size) = nameAndSize(uri)
    val dirFor = { root: String -> File(activity.cacheDir, root + "/" + sha1(uri.toString()).take(16)) }
    // Files Paginaut can always reach again are read where they are: no copy.
    if (lasting) {
      val dir = dirFor("direct").apply { deleteRecursively(); mkdirs() }
      openDirect(uri, display, dir)?.let { (link, pfd) -> return Opened(link, display, inPlace = true, lasting = true, kept = false, fp = fingerprint(pfd.fileDescriptor)) }
      // Not readable at random (a cloud file): copied, as a file lent once is.
    }
    // Big files lent once (a quarter of the cache or more) aren't kept: read in place, or failing
    // that copied for this visit only.
    if (!lasting && size > DIRECT_FROM) {
      val dir = dirFor("direct").apply { deleteRecursively(); mkdirs() }
      openDirect(uri, display, dir)?.let { (link, pfd) -> return Opened(link, display, inPlace = true, lasting = false, kept = false, fp = fingerprint(pfd.fileDescriptor)) }
      val out = File(dir, display)
      val input = cr.openInputStream(uri) ?: throw IllegalStateException("the file is empty")
      input.use { i -> FileOutputStream(out).use { o -> i.copyTo(o, 1 shl 16) } }
      synchronized(direct) { direct[out.absolutePath] = null }
      return Opened(out, display, inPlace = true, lasting = false, kept = false, fp = fingerprint(out))
    }
    val dir = dirFor("docs")
    val out = File(dir, display)
    if (size >= 0 && out.isFile && out.length() == size) {
      // Opened again: the copy counts as recently opened.
      dir.setLastModified(System.currentTimeMillis())
      return Opened(out, display, inPlace = false, lasting = lasting, kept = true, fp = fingerprint(out))
    }
    // Read the source before dropping the old copy: if it can't be read any more (a file another
    // app lent us for one visit), the copy is what Recent opens instead.
    val input = cr.openInputStream(uri) ?: throw IllegalStateException("the file is empty")
    dir.mkdirs()
    val tmp = File(dir, ".part")
    input.use { i -> FileOutputStream(tmp).use { o -> i.copyTo(o, 1 shl 16) } }
    dir.listFiles()?.forEach { if (it != tmp) it.delete() }
    if (!tmp.renameTo(out)) throw IllegalStateException("couldn't store a copy")
    return Opened(out, display, inPlace = false, lasting = lasting, kept = true, fp = fingerprint(out))
  }

  /**
   * Whether Paginaut can open `uri` again whenever it likes: picked here (Android keeps that
   * access), inside a folder chosen for the shelf, or a path while it has all-files access.
   */
  private fun canReach(uri: Uri): Boolean = when (uri.scheme) {
    "file" -> allFiles() && sharedBook(File(uri.path ?: ""))
    "content" -> {
      val s = uri.toString()
      activity.contentResolver.persistedUriPermissions.any { p ->
        p.isReadPermission && (p.uri == uri || (DocumentsContract.isTreeUri(p.uri) && s.startsWith(p.uri.toString() + "/document/")))
      }
    }
    else -> false
  }

  /**
   * A book on the phone's shared storage that a path may name (the shelf with all-files access): an
   * e-book or comic, not reached through a link, and not in another app's data or obb folder. The
   * engine checks the same again (mobile.rs, `readable`).
   */
  private fun sharedBook(f: File): Boolean {
    val root = Environment.getExternalStorageDirectory().canonicalPath + "/"
    val real = try { f.canonicalPath } catch (_: Exception) { return false }
    if (!real.startsWith(root) || real != f.absolutePath) return false
    val rest = real.removePrefix(root).lowercase()
    return !rest.startsWith("android/data/") && !rest.startsWith("android/obb/") && shelfKind(f.name) != null
  }

  /** Paginaut may read every file on the phone (the user turned on "All files access"). */
  private fun allFiles(): Boolean =
    if (Build.VERSION.SDK_INT >= 30) Environment.isExternalStorageManager()
    else activity.checkSelfPermission(Manifest.permission.READ_EXTERNAL_STORAGE) == PackageManager.PERMISSION_GRANTED

  /**
   * A file's contents in short: its size and its first and last 64 KB, hashed. The same file sent
   * again (WhatsApp and mail apps give it a new link each time), moved or renamed gives the same;
   * a changed one almost never does.
   */
  private fun fingerprint(fd: FileDescriptor): String = try {
    val size = Os.fstat(fd).st_size
    val md = MessageDigest.getInstance("SHA-1")
    md.update(size.toString().toByteArray())
    val buf = ByteArray(FP_PART)
    fun part(at: Long) {
      var got = 0
      while (got < buf.size) {
        val n = Os.pread(fd, buf, got, buf.size - got, at + got)
        if (n <= 0) break
        got += n
      }
      md.update(buf, 0, got)
    }
    part(0)
    if (size > FP_PART) part(maxOf(FP_PART.toLong(), size - FP_PART))
    md.digest().take(10).joinToString("") { "%02x".format(it) }
  } catch (_: Exception) {
    ""
  }

  private fun fingerprint(f: File): String = try {
    RandomAccessFile(f, "r").use { fingerprint(it.fd) }
  } catch (_: Exception) {
    ""
  }

  /**
   * What the app keeps and can reach: the copies in the cache (path, name, size, when last opened),
   * the files picked here, the folders chosen for the shelf, and whether it has all-files access.
   */
  @Command
  fun storage(invoke: Invoke) {
    if (MainActivity.refused) return invoke.reject(REFUSED)
    io.execute {
      val copies = JSONArray()
      val stale = System.currentTimeMillis() - 600_000
      File(activity.cacheDir, "docs").listFiles()?.filter { it.isDirectory }?.forEach { d ->
        // A folder with no copy (one that failed half-way, long ago) goes.
        val f = copyIn(d) ?: return@forEach run { if (d.lastModified() < stale) d.deleteRecursively() }
        copies.put(JSObject().put("path", f.absolutePath).put("name", f.name).put("size", f.length()).put("at", d.lastModified()))
      }
      val picked = JSONArray()
      val folders = JSONArray()
      for (p in activity.contentResolver.persistedUriPermissions) {
        if (!p.isReadPermission) continue
        if (DocumentsContract.isTreeUri(p.uri)) folders.put(JSObject().put("uri", p.uri.toString()).put("name", folderName(p.uri)))
        else picked.put(p.uri.toString())
      }
      invoke.resolve(JSObject().apply {
        put("copies", copies)
        put("picked", picked)
        put("folders", folders)
        put("shelfPicked", loadPicked().size)
        put("allFiles", allFiles())
      })
    }
  }

  /** A chosen folder's name as the Files app shows it. */
  private fun folderName(tree: Uri): String {
    val id = DocumentsContract.getTreeDocumentId(tree)
    return try {
      activity.contentResolver.query(DocumentsContract.buildDocumentUriUsingTree(tree, id), arrayOf(DocumentsContract.Document.COLUMN_DISPLAY_NAME), null, null, null)?.use { c ->
        if (c.moveToFirst()) c.getString(0) else null
      }
    } catch (_: Exception) {
      null
    } ?: id.substringAfterLast(':').ifEmpty { "Folder" }
  }

  /** Deletes these copies from the cache (the page decides which: records.ts). */
  @Command
  fun dropCopies(invoke: Invoke) {
    val arr = invoke.getArgs().optJSONArray("paths")
    val docs = File(activity.cacheDir, "docs").absolutePath + "/"
    val dirs = (0 until (arr?.length() ?: 0)).mapNotNull { File(arr!!.getString(it)).parentFile }.filter { it.absolutePath.startsWith(docs) }
    io.execute {
      dirs.forEach { it.deleteRecursively() }
      invoke.resolve()
    }
  }

  // ------------------------------------------------------------ the engine's process

  /** The current engine's binding: let go when a new engine is started (the old one stopped). */
  private var engineBinding: ServiceConnection? = null

  /**
   * Starts the reading engine in its isolated process (EngineService) and gives it one end of a new socket
   * pair: `{ fd }` is the app's end, detached for engine_link.rs. Called at start and whenever the engine
   * has stopped; the earlier binding goes first.
   */
  @Command
  fun engineConnect(invoke: Invoke) {
    if (MainActivity.refused) return invoke.reject(REFUSED)
    activity.runOnUiThread {
      engineBinding?.let {
        try {
          activity.unbindService(it)
        } catch (_: Exception) {
        }
      }
      val pair = ParcelFileDescriptor.createSocketPair()
      val binding = object : ServiceConnection {
        var done = false

        /** Not started (no binding, or none within the time below): said once, and the binding goes. */
        fun fail(why: String) {
          if (done) return
          done = true
          pair.forEach { it.close() }
          try {
            activity.unbindService(this)
          } catch (_: Exception) {
          }
          if (engineBinding === this) engineBinding = null
          invoke.reject(why)
        }

        override fun onServiceConnected(name: ComponentName, binder: IBinder) {
          // Android starts a stopped engine again for this binding: the app connects anew instead.
          if (done) return
          done = true
          try {
            Messenger(binder).send(Message.obtain().apply { data = Bundle().apply { putParcelable("socket", pair[1]) } })
            pair[1].close()
            invoke.resolve(JSObject().put("fd", pair[0].detachFd()))
          } catch (e: Exception) {
            pair.forEach { it.close() }
            invoke.reject("The reading engine didn't start: ${e.message}")
          }
        }

        override fun onServiceDisconnected(name: ComponentName) {}
        override fun onBindingDied(name: ComponentName) = fail("The reading engine didn't start.")
        override fun onNullBinding(name: ComponentName) = fail("The reading engine didn't start.")
      }
      engineBinding = binding
      // engine_link.rs waits for this: never forever.
      android.os.Handler(android.os.Looper.getMainLooper()).postDelayed({ binding.fail("The reading engine took too long to start.") }, 20_000)
      val started = try {
        activity.bindService(Intent(activity, EngineService::class.java), binding, Context.BIND_AUTO_CREATE or Context.BIND_IMPORTANT)
      } catch (_: Exception) {
        false
      }
      if (!started) binding.fail("The reading engine didn't start.")
    }
  }

  // ------------------------------------------------------------ the shelf

  /** Scans run apart from opening files (a whole phone takes a while). */
  private val scanner = Executors.newSingleThreadExecutor()

  /**
   * Asks for a folder for the shelf (Android's own picker); Paginaut keeps access to it. The picker
   * opens in Download, where most books land: Android 11 and later won't hand over Download itself,
   * but any folder inside it is fine.
   */
  @Command
  fun addFolder(invoke: Invoke) {
    if (MainActivity.refused) return invoke.reject(REFUSED)
    val pick = Intent(Intent.ACTION_OPEN_DOCUMENT_TREE)
    if (Build.VERSION.SDK_INT >= 26) pick.putExtra(DocumentsContract.EXTRA_INITIAL_URI, DOWNLOAD)
    try {
      startActivityForResult(invoke, pick, "folderPicked")
    } catch (e: ActivityNotFoundException) {
      invoke.reject("No folder picker is available on this device.")
    }
  }

  @ActivityCallback
  fun folderPicked(invoke: Invoke, result: ActivityResult) {
    val uri = result.data?.data
    if (result.resultCode != Activity.RESULT_OK || uri == null) return invoke.resolve(JSObject())
    try {
      activity.contentResolver.takePersistableUriPermission(uri, Intent.FLAG_GRANT_READ_URI_PERMISSION)
    } catch (_: SecurityException) {
      return invoke.reject("Android didn't let Paginaut keep this folder. Try another one.")
    }
    invoke.resolve(JSObject().put("uri", uri.toString()).put("name", folderName(uri)))
  }

  /**
   * "Search and pick books": Android's file picker, showing only e-books and comics (and files of no
   * known type: Android calls FB2 and CBT that), several at once. Paginaut keeps access to each book
   * picked and nothing else. Picking again only adds: books already on the shelf stay.
   */
  @Command
  fun pickBooks(invoke: Invoke) {
    if (MainActivity.refused) return invoke.reject(REFUSED)
    val pick = Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
      addCategory(Intent.CATEGORY_OPENABLE)
      type = "*/*"
      putExtra(Intent.EXTRA_MIME_TYPES, BOOK_TYPES)
      putExtra(Intent.EXTRA_ALLOW_MULTIPLE, true)
      if (Build.VERSION.SDK_INT >= 26) putExtra(DocumentsContract.EXTRA_INITIAL_URI, DOWNLOAD)
    }
    try {
      startActivityForResult(invoke, pick, "booksPicked")
    } catch (e: ActivityNotFoundException) {
      invoke.reject("No file picker is available on this device.")
    }
  }

  /** `{ added, already, notBooks, noRoom }`: how many books joined the shelf, and why others didn't. */
  @ActivityCallback
  fun booksPicked(invoke: Invoke, result: ActivityResult) {
    val data = result.data
    val none = JSObject().put("added", 0).put("already", 0).put("notBooks", 0).put("noRoom", 0)
    if (result.resultCode != Activity.RESULT_OK || data == null) return invoke.resolve(none)
    val uris = data.clipData?.let { c -> (0 until c.itemCount).mapNotNull { c.getItemAt(it).uri } } ?: listOfNotNull(data.data)
    io.execute {
      val cr = activity.contentResolver
      val kept = loadPicked()
      // Android keeps only so many files' access per app (512; 128 before Android 11), and past that
      // quietly drops the oldest: perhaps a chosen folder. A few places are left for files opened from Recent.
      var room = (if (Build.VERSION.SDK_INT >= 30) 512 else 128) - 16 - cr.persistedUriPermissions.size
      var added = 0
      var already = 0
      var notBooks = 0
      var noRoom = 0
      for (uri in uris) {
        if (shelfKind(nameAndSize(uri).first) == null) { notBooks++; continue }
        if (kept.contains(uri.toString())) { already++; continue }
        if (room <= 0) { noRoom++; continue }
        try {
          cr.takePersistableUriPermission(uri, Intent.FLAG_GRANT_READ_URI_PERMISSION)
        } catch (_: SecurityException) {
          noRoom++
          continue
        }
        room--
        kept.add(uri.toString())
        added++
      }
      savePicked(kept)
      invoke.resolve(JSObject().put("added", added).put("already", already).put("notBooks", notBooks).put("noRoom", noRoom))
    }
  }

  /**
   * Takes every picked book off the shelf: Paginaut gives its access back, except to files in Recent
   * (`keep`), which still open from there.
   */
  @Command
  fun forgetPicked(invoke: Invoke) {
    val keep = invoke.getArgs().optJSONArray("keep")?.let { a -> (0 until a.length()).map { a.getString(it) }.toSet() } ?: emptySet()
    io.execute {
      for (u in loadPicked()) {
        if (u in keep) continue
        try {
          activity.contentResolver.releasePersistableUriPermission(Uri.parse(u), Intent.FLAG_GRANT_READ_URI_PERMISSION)
        } catch (_: SecurityException) {
        }
      }
      savePicked(LinkedHashSet())
      invoke.resolve()
    }
  }

  /** The books picked for the shelf, by link, in the order they were picked. */
  private val pickedFile by lazy { File(activity.filesDir, "shelf-picked.json") }

  private fun loadPicked(): LinkedHashSet<String> = try {
    val a = JSONArray(pickedFile.readText())
    (0 until a.length()).mapTo(LinkedHashSet()) { a.getString(it) }
  } catch (_: Exception) {
    LinkedHashSet()
  }

  private fun savePicked(list: Set<String>) {
    try {
      pickedFile.writeText(JSONArray(list.toList()).toString())
    } catch (_: Exception) {
    }
  }

  /** Stops listing a folder on the shelf (Paginaut gives its access back). */
  @Command
  fun removeFolder(invoke: Invoke) {
    val uri = invoke.getArgs().getString("uri", null) ?: return invoke.resolve()
    try {
      activity.contentResolver.releasePersistableUriPermission(Uri.parse(uri), Intent.FLAG_GRANT_READ_URI_PERMISSION)
    } catch (_: SecurityException) {
    }
    invoke.resolve()
  }

  /**
   * Where "All files access" is turned on or off: Android's own page for it (Android 11 and later),
   * or, before that, its permission prompt (or the app's settings page, to turn it off).
   */
  @Command
  fun allFilesAccess(invoke: Invoke) {
    if (MainActivity.refused) return invoke.reject(REFUSED)
    val pkg = Uri.fromParts("package", activity.packageName, null)
    try {
      if (Build.VERSION.SDK_INT >= 30) {
        activity.startActivity(Intent(android.provider.Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION, pkg))
      } else if (!allFiles()) {
        activity.requestPermissions(arrayOf(Manifest.permission.READ_EXTERNAL_STORAGE), 41)
      } else {
        activity.startActivity(Intent(android.provider.Settings.ACTION_APPLICATION_DETAILS_SETTINGS, pkg))
      }
    } catch (e: ActivityNotFoundException) {
      activity.startActivity(Intent(android.provider.Settings.ACTION_APPLICATION_DETAILS_SETTINGS, pkg))
    }
    invoke.resolve()
  }

  /** What each file looked like when last scanned, so unchanged ones aren't read again. */
  private data class Seen(val size: Long, val mtime: Long, val fp: String)

  private val indexFile by lazy { File(activity.filesDir, "shelf-index.json") }

  private fun loadIndex(): MutableMap<String, Seen> = try {
    val o = org.json.JSONObject(indexFile.readText())
    o.keys().asSequence().associateWith { k -> o.getJSONArray(k).let { Seen(it.getLong(0), it.getLong(1), it.getString(2)) } }.toMutableMap()
  } catch (_: Exception) {
    mutableMapOf()
  }

  /**
   * The e-books and comics in the folders chosen for the shelf, and (with all-files access) anywhere
   * on the phone: `{ items: [{ id, name, size, mtime, fp, kind }] }`. `id` opens it with `reopen`.
   * Each file's fingerprint (to know it whichever way it was opened before) is read once.
   */
  @Command
  fun scanBooks(invoke: Invoke) {
    if (MainActivity.refused) return invoke.reject(REFUSED)
    scanner.execute {
      val before = loadIndex()
      val now = HashMap<String, Seen>()
      val items = JSONArray()
      val fps = HashSet<String>()
      fun add(id: String, name: String, size: Long, mtime: Long, fp: () -> String) {
        val kind = shelfKind(name) ?: return
        val seen = before[id]?.takeIf { it.size == size && it.mtime == mtime } ?: Seen(size, mtime, fp())
        now[id] = seen
        // The same book in two places (a chosen folder inside the phone's storage) is listed once.
        if (seen.fp.isNotEmpty() && !fps.add(seen.fp)) return
        items.put(JSObject().put("id", id).put("name", name).put("size", size).put("mtime", mtime).put("fp", seen.fp).put("kind", kind))
      }
      try {
        if (allFiles()) {
          val root = Environment.getExternalStorageDirectory()
          fun walk(dir: File, depth: Int) {
            val list = dir.listFiles() ?: return
            for (f in list) {
              if (f.name.startsWith(".")) continue
              if (f.isDirectory) {
                // In Android's own folder only media: where WhatsApp and others keep files they
                // received (Android/media/com.whatsapp/WhatsApp/Media/WhatsApp Documents). Other
                // apps' data and obb folders are theirs alone (Android refuses them anyway).
                if (depth == 0 && f.name == "Android") File(f, "media").takeIf { it.isDirectory }?.let { walk(it, 2) }
                else if (depth < 12) walk(f, depth + 1)
              } else add(Uri.fromFile(f).toString(), f.name, f.length(), f.lastModified()) { fingerprint(f) }
            }
          }
          walk(root, 0)
        }
        val cr = activity.contentResolver
        for (p in cr.persistedUriPermissions) {
          if (!p.isReadPermission || !DocumentsContract.isTreeUri(p.uri)) continue
          val tree = p.uri
          fun walk(docId: String, depth: Int) {
            val children = DocumentsContract.buildChildDocumentsUriUsingTree(tree, docId)
            val cols = arrayOf(
              DocumentsContract.Document.COLUMN_DOCUMENT_ID, DocumentsContract.Document.COLUMN_DISPLAY_NAME,
              DocumentsContract.Document.COLUMN_MIME_TYPE, DocumentsContract.Document.COLUMN_SIZE, DocumentsContract.Document.COLUMN_LAST_MODIFIED,
            )
            val found = ArrayList<Array<Any?>>()
            try {
              cr.query(children, cols, null, null, null)?.use { c ->
                while (c.moveToNext()) found.add(arrayOf(c.getString(0), c.getString(1), c.getString(2), if (c.isNull(3)) -1L else c.getLong(3), if (c.isNull(4)) 0L else c.getLong(4)))
              }
            } catch (_: Exception) {
              return
            }
            for ((id, name, mime, size, mtime) in found) {
              val n = name as? String ?: continue
              if (n.startsWith(".")) continue
              if (mime == DocumentsContract.Document.MIME_TYPE_DIR) {
                if (depth < 12) walk(id as String, depth + 1)
              } else if (shelfKind(n) != null) {
                val uri = DocumentsContract.buildDocumentUriUsingTree(tree, id as String)
                add(uri.toString(), n, size as Long, mtime as Long) {
                  try {
                    cr.openFileDescriptor(uri, "r")?.use { fingerprint(it.fileDescriptor) } ?: ""
                  } catch (_: Exception) {
                    ""
                  }
                }
              }
            }
          }
          try {
            walk(DocumentsContract.getTreeDocumentId(tree), 0)
          } catch (_: Exception) {
          }
        }
        // Books picked one by one ("Search and pick books"). One that's gone (deleted, or its access
        // taken back in Android's settings) leaves the list; the rest stay as they are.
        val picked = loadPicked()
        val granted = cr.persistedUriPermissions.filter { it.isReadPermission }.map { it.uri.toString() }.toSet()
        val gone = HashSet<String>()
        for (u in picked) {
          if (u !in granted) { gone.add(u); continue }
          val uri = Uri.parse(u)
          try {
            val cols = arrayOf(OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE, DocumentsContract.Document.COLUMN_LAST_MODIFIED)
            val row = cr.query(uri, cols, null, null, null)?.use { c ->
              if (!c.moveToFirst()) null
              else Triple(c.getString(0), if (c.isNull(1)) -1L else c.getLong(1), c.getColumnIndex(cols[2]).let { i -> if (i < 0 || c.isNull(i)) 0L else c.getLong(i) })
            }
            val name = row?.first
            if (row == null || name == null) { gone.add(u); continue }
            add(u, name, row.second, row.third) {
              try {
                cr.openFileDescriptor(uri, "r")?.use { fingerprint(it.fileDescriptor) } ?: ""
              } catch (_: Exception) {
                ""
              }
            }
          } catch (_: java.io.FileNotFoundException) {
            gone.add(u)
          } catch (_: Exception) {
          }
        }
        if (gone.isNotEmpty()) savePicked(picked.filterTo(LinkedHashSet()) { it !in gone })
        try {
          indexFile.writeText(org.json.JSONObject(now.mapValues { (_, s) -> JSONArray(listOf(s.size, s.mtime, s.fp)) }).toString())
        } catch (_: Exception) {
        }
        invoke.resolve(JSObject().put("items", items))
      } catch (e: Exception) {
        invoke.reject("Couldn't look for books: ${e.message ?: e.javaClass.simpleName}")
      }
    }
  }

  /** E-book or comic, by extension (what the shelf lists); null for anything else. */
  private fun shelfKind(name: String): String? = when (name.substringAfterLast('.', "").lowercase()) {
    "epub", "mobi", "fb2", "fbz" -> "ebook"
    "cbz", "cbr", "cbt" -> "comic"
    else -> null
  }

  // ------------------------------------------------------------ sharing and printing

  @Command
  fun share(invoke: Invoke) {
    val args = invoke.getArgs()
    val path = args.getString("path", null) ?: return invoke.reject("No file given.")
    val file = File(path)
    // A big file read in place is shared from where it is (its link isn't a file to hand on).
    val source = args.getString("source", null)
    val inPlace = synchronized(direct) { direct.containsKey(path) }
    val uri = if (inPlace && source != null) Uri.parse(source) else try {
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

  /** Shares text (a selection on a page) through Android's share sheet. */
  @Command
  fun shareText(invoke: Invoke) {
    val text = invoke.getArgs().getString("text", null) ?: return invoke.reject("Nothing to share.")
    val send = Intent(Intent.ACTION_SEND).apply {
      type = "text/plain"
      putExtra(Intent.EXTRA_TEXT, text)
    }
    activity.startActivity(Intent.createChooser(send, null))
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
    // A big file read in place: Android lent a handle, and opening its link again is refused; read through
    // a handle of its own (reads from the start: everyone else reading it uses positioned reads).
    val held = synchronized(direct) { direct[path] }
    val open = {
      if (held != null) {
        val own = ParcelFileDescriptor.dup(held.fileDescriptor)
        Os.lseek(own.fileDescriptor, 0, OsConstants.SEEK_SET)
        ParcelFileDescriptor.AutoCloseInputStream(own)
      } else FileInputStream(File(path))
    }
    activity.runOnUiThread {
      pm.print(title, FileAdapter(open, title), PrintAttributes.Builder().build())
      invoke.resolve()
    }
  }

  private class FileAdapter(private val open: () -> java.io.InputStream, private val title: String) : PrintDocumentAdapter() {
    override fun onLayout(old: PrintAttributes?, new: PrintAttributes?, cancel: CancellationSignal?, cb: LayoutResultCallback, extras: Bundle?) {
      if (cancel?.isCanceled == true) return cb.onLayoutCancelled()
      val info = PrintDocumentInfo.Builder("$title.pdf").setContentType(PrintDocumentInfo.CONTENT_TYPE_DOCUMENT).build()
      cb.onLayoutFinished(info, old != new)
    }

    override fun onWrite(pages: Array<out PageRange>?, dest: ParcelFileDescriptor, cancel: CancellationSignal?, cb: WriteResultCallback) {
      try {
        open().use { i -> FileOutputStream(dest.fileDescriptor).use { o -> i.copyTo(o, 1 shl 16) } }
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
  /**
   * Hide from screenshots: Android's secure window. Screenshots, screen recordings, shared and cast
   * screens show black, and Recent apps no preview. Remembered for the next start (MainActivity sets it
   * before the first frame).
   */
  @Command
  fun setSecure(invoke: Invoke) {
    val on = invoke.getArgs().optBoolean("on", false)
    MainActivity.rememberSecure(activity, on)
    activity.runOnUiThread {
      MainActivity.applySecure(activity, on)
      invoke.resolve()
    }
  }

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
    private const val CACHE_LIMIT = 512L shl 20
    /** Files this big are read in place rather than copied into the cache. */
    private const val DIRECT_FROM = CACHE_LIMIT / 4
    /** How much of each end of a file its fingerprint reads. */
    private const val FP_PART = 64 * 1024

    /** What the picker offers: PDF, XPS, e-books, comics, Markdown, pictures (and unnamed files, as many comics are). */
    const val REFUSED = "Paginaut stopped: this copy asks for permissions it doesn't use."

    /** The Download folder, where the pickers start. */
    val DOWNLOAD: Uri = DocumentsContract.buildDocumentUri("com.android.externalstorage.documents", "primary:Download")

    /** What "Search and pick books" shows: e-books and comics, and files of no known type (FB2, FBZ, CBT on Android). */
    val BOOK_TYPES = arrayOf(
      "application/epub+zip", "application/x-mobipocket-ebook", "application/x-fictionbook+xml", "application/x-fictionbook",
      "application/vnd.comicbook+zip", "application/x-cbz", "application/x-cbr", "application/vnd.comicbook-rar",
      "application/octet-stream",
    )

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
      "cbr" to "application/vnd.comicbook-rar",
      "md" to "text/markdown",
    )
  }
}
