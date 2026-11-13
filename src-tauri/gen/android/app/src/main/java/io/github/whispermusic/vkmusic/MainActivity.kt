package io.github.whispermusic.vkmusic

import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import android.os.Environment
import android.system.Os
import android.view.View
import android.webkit.WebView
import androidx.activity.enableEdgeToEdge
import androidx.core.app.ActivityCompat
import androidx.core.content.ContextCompat
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat

/**
 * Окно приложения. До запуска Rust (он стартует из super.onCreate) — папки: HOME и XDG_* — личная папка приложения
 * (настройки, библиотека, кэш; config.rs, dirs), VKM_MUSIC_DIR / VKM_DOWNLOAD_DIR — общие «Музыка» и «Загрузки»
 * телефона (папка музыки и «Скачать»).
 */
class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    val files = filesDir.absolutePath
    Os.setenv("HOME", files, true)
    Os.setenv("XDG_CONFIG_HOME", "$files/config", true)
    Os.setenv("XDG_DATA_HOME", "$files/data", true)
    Os.setenv("XDG_CACHE_HOME", cacheDir.absolutePath, true)
    Os.setenv("XDG_RUNTIME_DIR", cacheDir.absolutePath, true)
    @Suppress("DEPRECATION")
    Os.setenv("VKM_MUSIC_DIR", Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_MUSIC).absolutePath, true)
    @Suppress("DEPRECATION")
    Os.setenv("VKM_DOWNLOAD_DIR", Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOWNLOADS).absolutePath, true)
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    // интерфейс — между системными панелями и над клавиатурой (Android 15+ рисует приложение под ними)
    val root = findViewById<View>(android.R.id.content)
    ViewCompat.setOnApplyWindowInsetsListener(root) { v, insets ->
      val b = insets.getInsets(
        WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout() or WindowInsetsCompat.Type.ime()
      )
      v.setPadding(b.left, b.top, b.right, b.bottom)
      WindowInsetsCompat.CONSUMED
    }
    askPermissions()
  }

  /** Уведомление плеера (Android 13+), чтение музыки телефона, запись в «Загрузки» на Android 9 и старше. */
  private fun askPermissions() {
    val want = mutableListOf<String>()
    if (Build.VERSION.SDK_INT >= 33) {
      want += Manifest.permission.POST_NOTIFICATIONS
      want += Manifest.permission.READ_MEDIA_AUDIO
    } else {
      want += Manifest.permission.READ_EXTERNAL_STORAGE
      if (Build.VERSION.SDK_INT <= 28) want += Manifest.permission.WRITE_EXTERNAL_STORAGE
    }
    val missing = want.filter { ContextCompat.checkSelfPermission(this, it) != PackageManager.PERMISSION_GRANTED }
    if (missing.isNotEmpty()) ActivityCompat.requestPermissions(this, missing.toTypedArray(), 1)
  }

  override fun onWebViewCreate(webView: WebView) {
    // музыка играет с выключенным экраном: следующий трек включает интерфейс (событие ended), поэтому процесс
    // страницы не должен засыпать вместе с окном
    if (Build.VERSION.SDK_INT >= 26) webView.setRendererPriorityPolicy(WebView.RENDERER_PRIORITY_IMPORTANT, false)
  }
}
