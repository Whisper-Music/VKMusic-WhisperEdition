package io.github.whispermusic.vkmusic

import android.annotation.SuppressLint
import android.app.Activity
import android.app.Dialog
import android.content.ComponentName
import android.content.Intent
import android.graphics.Color
import android.net.Uri
import android.view.Gravity
import android.view.ViewGroup
import android.view.WindowManager
import android.webkit.CookieManager
import android.webkit.WebResourceRequest
import android.webkit.WebView
import android.webkit.WebViewClient
import android.widget.Button
import android.widget.LinearLayout
import android.widget.TextView
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.media3.common.util.UnstableApi
import androidx.media3.session.MediaController
import androidx.media3.session.SessionToken
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Channel
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import com.google.common.util.concurrent.ListenableFuture

@InvokeArg
class EventsArgs {
  lateinit var channel: Channel
}

@InvokeArg
class LoadArgs {
  var src: String = ""
  var title: String = ""
  var userAgent: String = ""
  var referer: String = ""
  var start: Double = 0.0
  var pause: Boolean = false
}

@InvokeArg
class MetaArgs {
  var title: String = ""
  var artist: String = ""
  var cover: String = ""
  var playing: Boolean = false
}

@InvokeArg
class EffectsArgs {
  var eq: Boolean = false
  var pre: Double = 0.0
  var gains: DoubleArray = DoubleArray(10)
  var normalize: Boolean = false
}

@InvokeArg
class LoginArgs {
  var url: String = ""
  var title: String = ""
  var cancel: String = "Отмена"
  var userAgent: String = ""
}

@InvokeArg
class UrlsArgs {
  var urls: Array<String> = emptyArray()
}

@InvokeArg
class UrlArgs {
  var url: String = ""
}

@InvokeArg
class BarsArgs {
  var bg: String = ""
}

/**
 * Kotlin-часть VK Music (mobile/mod.rs): плеер (Engine, PlaybackService), страница входа в сервисы, куки
 * CookieManager, ссылки, цвет системных панелей. Команды зовёт только Rust (`run_mobile_plugin_async`), не интерфейс.
 * Все команды выполняются в главном потоке Android.
 */
@UnstableApi
@TauriPlugin
class VkmPlugin(private val activity: Activity) : Plugin(activity) {
  private var controller: ListenableFuture<MediaController>? = null
  private var login: Dialog? = null
  private var loginWeb: WebView? = null

  /** Связь со службой плеера: пока она есть, служба жива и показывает уведомление. */
  private fun connect() {
    if (controller != null) return
    val token = SessionToken(activity, ComponentName(activity, PlaybackService::class.java))
    controller = MediaController.Builder(activity, token).buildAsync()
  }

  override fun onDestroy(activity: androidx.appcompat.app.AppCompatActivity) {
    controller?.let { MediaController.releaseFuture(it) }
    controller = null
    login?.dismiss()
  }

  // ───────────── плеер ─────────────

  @Command
  fun setEvents(invoke: Invoke) {
    val ch = invoke.parseArgs(EventsArgs::class.java).channel
    val e = Engine.get(activity)
    e.sink = { ch.send(it) }
    // первое состояние сразу, как mpv после запуска: по нему интерфейс рисует кнопку «Пуск»
    e.push()
    invoke.resolve()
  }

  @Command
  fun load(invoke: Invoke) {
    val a = invoke.parseArgs(LoadArgs::class.java)
    connect()
    Engine.get(activity).load(a.src, a.title, a.userAgent, a.referer, a.start, a.pause)
    invoke.resolve()
  }

  @Command
  fun meta(invoke: Invoke) {
    val a = invoke.parseArgs(MetaArgs::class.java)
    Engine.get(activity).meta(a.title, a.artist, a.cover)
    invoke.resolve()
  }

  @Command
  fun pause(invoke: Invoke) {
    Engine.get(activity).pause(invoke.getArgs().getBoolean("paused", true))
    invoke.resolve()
  }

  @Command
  fun seek(invoke: Invoke) {
    Engine.get(activity).seek(invoke.getArgs().getDouble("pos"))
    invoke.resolve()
  }

  @Command
  fun stop(invoke: Invoke) {
    Engine.get(activity).stop()
    invoke.resolve()
  }

  @Command
  fun volume(invoke: Invoke) {
    Engine.get(activity).volume(invoke.getArgs().getDouble("gain").toFloat())
    invoke.resolve()
  }

  @Command
  fun speed(invoke: Invoke) {
    Engine.get(activity).speed(invoke.getArgs().getDouble("rate").toFloat())
    invoke.resolve()
  }

  @Command
  fun effects(invoke: Invoke) {
    val a = invoke.parseArgs(EffectsArgs::class.java)
    val g = FloatArray(10) { i -> a.gains.getOrElse(i) { 0.0 }.toFloat() }
    Engine.get(activity).effects(a.eq, a.pre.toFloat(), g, a.normalize)
    invoke.resolve()
  }

  // ───────────── вход в сервисы ─────────────

  /** Страница входа поверх приложения: свой WebView (общие куки CookieManager), без связи с командами приложения. */
  @SuppressLint("SetJavaScriptEnabled")
  @Command
  fun loginOpen(invoke: Invoke) {
    val a = invoke.parseArgs(LoginArgs::class.java)
    val url = Uri.parse(a.url)
    if (url.scheme != "https") {
      invoke.reject("Неверный адрес страницы входа")
      return
    }
    login?.dismiss()
    val d = Dialog(activity, android.R.style.Theme_DeviceDefault_NoActionBar)
    val web = WebView(activity)
    web.settings.apply {
      javaScriptEnabled = true
      domStorageEnabled = true
      if (a.userAgent.isNotEmpty()) userAgentString = a.userAgent
      setSupportMultipleWindows(false) // всплывающие окна (VK ID) — в этой же странице
      javaScriptCanOpenWindowsAutomatically = true
    }
    CookieManager.getInstance().setAcceptCookie(true)
    CookieManager.getInstance().setAcceptThirdPartyCookies(web, true)
    web.webViewClient = object : WebViewClient() {
      // только веб-страницы: ссылки на приложения (intent:, vk://) не открываем
      override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest): Boolean {
        val s = request.url.scheme
        return s != "https" && s != "http"
      }
    }
    val bar = LinearLayout(activity).apply {
      orientation = LinearLayout.HORIZONTAL
      gravity = Gravity.CENTER_VERTICAL
      setBackgroundColor(Color.rgb(0x16, 0x16, 0x1c))
      setPadding(32, 8, 8, 8)
      addView(TextView(activity).apply {
        text = a.title
        setTextColor(Color.WHITE)
        textSize = 16f
      }, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
      addView(Button(activity).apply {
        text = a.cancel
        setOnClickListener { d.dismiss() }
      })
    }
    val root = LinearLayout(activity).apply {
      orientation = LinearLayout.VERTICAL
      setBackgroundColor(Color.rgb(0x16, 0x16, 0x1c))
      addView(bar, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))
      addView(web, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))
    }
    ViewCompat.setOnApplyWindowInsetsListener(root) { v, insets ->
      val b = insets.getInsets(
        WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout() or WindowInsetsCompat.Type.ime()
      )
      v.setPadding(b.left, b.top, b.right, b.bottom)
      WindowInsetsCompat.CONSUMED
    }
    d.setContentView(root)
    d.window?.let {
      it.setLayout(WindowManager.LayoutParams.MATCH_PARENT, WindowManager.LayoutParams.MATCH_PARENT)
      WindowCompat.setDecorFitsSystemWindows(it, false)
    }
    d.setOnDismissListener {
      CookieManager.getInstance().flush()
      web.destroy()
      if (login === d) {
        login = null
        loginWeb = null
      }
    }
    login = d
    loginWeb = web
    d.show()
    web.loadUrl(a.url)
    invoke.resolve()
  }

  @Command
  fun loginState(invoke: Invoke) {
    val open = login?.isShowing == true
    invoke.resolve(JSObject().put("open", open).put("url", if (open) loginWeb?.url ?: "" else ""))
  }

  @Command
  fun loginNavigate(invoke: Invoke) {
    val url = invoke.parseArgs(UrlArgs::class.java).url
    if (Uri.parse(url).scheme == "https") loginWeb?.loadUrl(url)
    invoke.resolve()
  }

  @Command
  fun loginClose(invoke: Invoke) {
    login?.dismiss()
    invoke.resolve()
  }

  // ───────────── куки ─────────────

  /** Строка кук («имя=значение; …») для каждого адреса. */
  @Command
  fun cookies(invoke: Invoke) {
    val cm = CookieManager.getInstance()
    val out = JSObject()
    for (u in invoke.parseArgs(UrlsArgs::class.java).urls) {
      if (Uri.parse(u).scheme == "https") out.put(u, cm.getCookie(u) ?: "")
    }
    invoke.resolve(out)
  }

  /** Выход: каждой куке адреса — истёкшая с тем же именем (и для сайта целиком, и для хоста). */
  @Command
  fun cookiesDelete(invoke: Invoke) {
    val cm = CookieManager.getInstance()
    for (u in invoke.parseArgs(UrlsArgs::class.java).urls) {
      val uri = Uri.parse(u)
      if (uri.scheme != "https") continue
      val host = uri.host ?: continue
      val names = (cm.getCookie(u) ?: "").split(';').mapNotNull { it.substringBefore('=').trim().ifEmpty { null } }
      for (n in names.distinct()) {
        val gone = "$n=; Max-Age=0; Expires=Thu, 01 Jan 1970 00:00:00 GMT; Path=/; Secure"
        cm.setCookie(u, gone)
        cm.setCookie(u, "$gone; Domain=.$host")
      }
    }
    cm.flush()
    invoke.resolve(JSObject())
  }

  // ───────────── система ─────────────

  @Command
  fun openUrl(invoke: Invoke) {
    val url = invoke.parseArgs(UrlArgs::class.java).url
    val uri = Uri.parse(url)
    if (uri.scheme == "https" || uri.scheme == "http") {
      try {
        activity.startActivity(Intent(Intent.ACTION_VIEW, uri).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
      } catch (_: Exception) {
      }
    }
    invoke.resolve()
  }

  /** «Назад» на главном экране — свернуть, а не закрыть: музыка играет дальше, как у других плееров. */
  @Command
  fun moveToBack(invoke: Invoke) {
    activity.moveTaskToBack(true)
    invoke.resolve()
  }

  /** Цвет фона темы — под системные панели (видны по краям интерфейса), значки панелей — светлые или тёмные. */
  @Command
  fun bars(invoke: Invoke) {
    val bg = invoke.parseArgs(BarsArgs::class.java).bg
    val c = try {
      Color.parseColor(bg)
    } catch (_: Exception) {
      invoke.resolve()
      return
    }
    // запомнить: при следующем запуске окно сразу этого цвета (MainActivity), без цвета прошлой темы
    activity.getSharedPreferences(MainActivity.PREFS, Activity.MODE_PRIVATE).edit().putInt(MainActivity.PREF_BG, c).apply()
    MainActivity.applyBars(activity, c)
    invoke.resolve()
  }
}
