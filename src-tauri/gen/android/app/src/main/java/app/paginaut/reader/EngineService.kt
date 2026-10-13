package app.paginaut.reader

import android.app.Service
import android.content.Intent
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.os.Messenger
import android.os.ParcelFileDescriptor

/**
 * The reading engine (MuPDF, which reads every document), in its own isolated process: no permissions, no
 * files, no network (android:isolatedProcess in the manifest). The app sends it one end of a socket pair
 * (PaginautPlugin.engineConnect); everything else, documents included, comes over that socket as open
 * file handles (engine_link.rs).
 */
class EngineService : Service() {
  private val messenger = Messenger(Handler(Looper.getMainLooper()) { msg ->
    @Suppress("DEPRECATION")
    val socket = msg.data?.getParcelable<ParcelFileDescriptor>("socket")
    if (socket != null) {
      val fd = socket.detachFd()
      Thread({ serve(fd) }, "engine").start()
    }
    true
  })

  override fun onBind(intent: Intent): IBinder = messenger.binder

  /** Serves the app over the socket `fd` until the app goes (engine_link.rs). */
  private external fun serve(fd: Int)

  companion object {
    init {
      System.loadLibrary("paginaut_lib")
    }
  }
}
