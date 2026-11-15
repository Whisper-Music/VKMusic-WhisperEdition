package io.github.whispermusic.vkmusic

import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import android.os.Environment
import android.system.Os
import android.app.Activity
import android.graphics.Color
import android.graphics.drawable.ColorDrawable
import android.view.View
import android.webkit.WebView
import androidx.activity.enableEdgeToEdge
import androidx.core.app.ActivityCompat
import androidx.core.content.ContextCompat
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat

/**
 * Окно приложения. До запуска Rust (он стартует из super.onCreate) — папки: HOME и XDG_* — личная папка приложения
 * (настройки, библиотека, кэш; config.rs, dirs), VKM_MUSIC_DIR — своя папка «Музыка/VK Music» (как ~/Music/vk на
 * компьютере: в библиотеку попадает только то, что положили туда, а не вся музыка телефона — просьба владельца,
 * 2026-11-14), VKM_DOWNLOAD_DIR — общие «Загрузки» («Скачать»).
 */
class MainActivity : TauriActivity() {
  companion object {
    const val PREFS = "vkm"
    /** фон темы прошлого запуска (VkmPlugin.bars) — окно и системные панели до загрузки интерфейса */
    const val PREF_BG = "bg"

    /** Окно, строка состояния и навигация — цветом фона темы; значки панелей — тёмные на светлом. */
    fun applyBars(activity: Activity, c: Int) {
      activity.window.setBackgroundDrawable(ColorDrawable(c))
      activity.findViewById<View>(android.R.id.content)?.setBackgroundColor(c)
      val light = (0.299 * Color.red(c) + 0.587 * Color.green(c) + 0.114 * Color.blue(c)) > 150
      WindowCompat.getInsetsController(activity.window, activity.window.decorView).apply {
        isAppearanceLightStatusBars = light
        isAppearanceLightNavigationBars = light
      }
    }
  }

  override fun onCreate(savedInstanceState: Bundle?) {
    val bg = getSharedPreferences(PREFS, MODE_PRIVATE).let { if (it.contains(PREF_BG)) it.getInt(PREF_BG, 0) else null }
    if (bg != null) window.setBackgroundDrawable(ColorDrawable(bg))
    val files = filesDir.absolutePath
    Os.setenv("HOME", files, true)
    Os.setenv("XDG_CONFIG_HOME", "$files/config", true)
    Os.setenv("XDG_DATA_HOME", "$files/data", true)
    Os.setenv("XDG_CACHE_HOME", cacheDir.absolutePath, true)
    Os.setenv("XDG_RUNTIME_DIR", cacheDir.absolutePath, true)
    @Suppress("DEPRECATION")
    val music = java.io.File(Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_MUSIC), "VK Music")
    // папка — сразу, чтобы её было видно в «Файлах» и было куда класть свои треки
    try { music.mkdirs() } catch (_: Exception) {}
    Os.setenv("VKM_MUSIC_DIR", music.absolutePath, true)
    @Suppress("DEPRECATION")
    Os.setenv("VKM_DOWNLOAD_DIR", Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOWNLOADS).absolutePath, true)
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    if (bg != null) applyBars(this, bg)
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
    // масштаб двумя пальцами не нужен — это приложение, а не страница (и viewport в boot.js)
    webView.settings.setSupportZoom(false)
    webView.settings.builtInZoomControls = false
    webView.settings.displayZoomControls = false
  }
}
