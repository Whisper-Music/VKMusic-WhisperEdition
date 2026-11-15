package io.github.whispermusic.vkmusic

import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.media.audiofx.DynamicsProcessing
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import androidx.media3.common.AudioAttributes
import androidx.media3.common.C
import androidx.media3.common.ForwardingPlayer
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.common.MimeTypes
import androidx.media3.common.PlaybackException
import androidx.media3.common.Player
import androidx.media3.common.util.UnstableApi
import androidx.media3.datasource.DefaultDataSource
import androidx.media3.datasource.DefaultHttpDataSource
import androidx.media3.exoplayer.DefaultRenderersFactory
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.exoplayer.audio.AudioSink
import androidx.media3.exoplayer.audio.DefaultAudioSink
import androidx.media3.exoplayer.audio.TeeAudioProcessor
import androidx.media3.exoplayer.hls.HlsMediaSource
import androidx.media3.exoplayer.source.MediaSource
import androidx.media3.exoplayer.source.ProgressiveMediaSource
import androidx.media3.session.CommandButton
import androidx.media3.session.DefaultMediaNotificationProvider
import androidx.media3.session.MediaSession
import androidx.media3.session.MediaSessionService
import androidx.media3.session.SessionCommand
import androidx.media3.session.SessionResult
import com.google.common.collect.ImmutableList
import com.google.common.util.concurrent.Futures
import com.google.common.util.concurrent.ListenableFuture
import app.tauri.plugin.JSObject
import java.io.File

/**
 * Плеер приложения (как mpv на компьютере — один на процесс): ExoPlayer, события для Rust (mobile/player.rs),
 * эквалайзер и выравнивание громкости (DynamicsProcessing, Android 9+).
 *
 * Состояние уходит событиями `state` (как observe_property mpv): при каждом изменении и 4 раза в секунду, пока
 * играет. `file` растёт, когда загруженный трек готов (как file-loaded у mpv): интерфейс по нему отличает новый трек
 * от запоздавших событий прошлого.
 */
@UnstableApi
object Engine {
  private val main = Handler(Looper.getMainLooper())
  private var ready = false
  lateinit var exo: ExoPlayer
    private set
  /** ExoPlayer для уведомления и экрана блокировки: «назад/вперёд» — интерфейсу (очередь — в нём). */
  lateinit var session: Player
    private set
  private lateinit var app: Context

  /** события в Rust (VkmPlugin.setEvents) */
  var sink: ((JSObject) -> Unit)? = null
  private var file = 0L
  private var loading = false
  /** текущий трек: для замены подписи и обложки без перезагрузки и для повтора как HLS */
  private var item: MediaItem? = null
  private var headers: Pair<String, String> = "" to ""
  private var triedHls = false

  // эффекты
  private var dp: DynamicsProcessing? = null
  private var dpSession = C.AUDIO_SESSION_ID_UNSET
  private var eqOn = false
  private var pre = 0f
  private var gains = FloatArray(10)
  private var norm = false

  fun get(ctx: Context): Engine {
    if (ready) return this
    app = ctx.applicationContext
    val attrs = AudioAttributes.Builder().setUsage(C.USAGE_MEDIA).setContentType(C.AUDIO_CONTENT_TYPE_MUSIC).build()
    // ответвитель звука для визуализатора (Viz.kt): пока он выключен, только считает отсчёты
    val renderers = object : DefaultRenderersFactory(app) {
      override fun buildAudioSink(context: Context, enableFloatOutput: Boolean, enableAudioTrackPlaybackParams: Boolean): AudioSink =
        DefaultAudioSink.Builder(context)
          .setEnableFloatOutput(enableFloatOutput)
          .setEnableAudioTrackPlaybackParams(enableAudioTrackPlaybackParams)
          .setAudioProcessors(arrayOf(TeeAudioProcessor(Viz)))
          .build()
    }
    exo = ExoPlayer.Builder(app, renderers)
      .setAudioAttributes(attrs, true) // звонок, навигатор, другое приложение — пауза/тише сами
      .setHandleAudioBecomingNoisy(true) // наушники вынули — пауза
      .setWakeMode(C.WAKE_MODE_NETWORK)
      .build()
    exo.addListener(object : Player.Listener {
      override fun onPlaybackStateChanged(state: Int) {
        if (state == Player.STATE_READY && loading) {
          loading = false
          file++
        }
        if (state == Player.STATE_ENDED) emit(JSObject().put("type", "ended"))
        push()
      }

      override fun onPlayWhenReadyChanged(playWhenReady: Boolean, reason: Int) = push()
      override fun onIsPlayingChanged(isPlaying: Boolean) {
        push()
        if (isPlaying) tick()
      }

      override fun onPlayerError(error: PlaybackException) {
        // поток без расширения в адресе (ВК API) может оказаться HLS — один раз пробуем так
        val cur = item
        if (!triedHls && cur != null && error.errorCode == PlaybackException.ERROR_CODE_PARSING_CONTAINER_UNSUPPORTED) {
          triedHls = true
          val uri = cur.localConfiguration?.uri ?: return
          if (uri.scheme == "https") {
            open(cur.buildUpon().setMimeType(MimeTypes.APPLICATION_M3U8).build(), exo.currentPosition, !exo.playWhenReady)
            return
          }
        }
        loading = false
        val why = error.cause?.message?.take(200)
        emit(JSObject().put("type", "error").put("message", if (why.isNullOrBlank()) error.errorCodeName else "${error.errorCodeName}: $why"))
        push()
      }

      override fun onAudioSessionIdChanged(audioSessionId: Int) = applyEffects()
    })
    session = object : ForwardingPlayer(exo) {
      private val extra = listOf(
        Player.COMMAND_SEEK_TO_NEXT, Player.COMMAND_SEEK_TO_NEXT_MEDIA_ITEM,
        Player.COMMAND_SEEK_TO_PREVIOUS, Player.COMMAND_SEEK_TO_PREVIOUS_MEDIA_ITEM,
      )

      override fun getAvailableCommands(): Player.Commands =
        super.getAvailableCommands().buildUpon().addAll(*extra.toIntArray()).build()

      override fun isCommandAvailable(command: Int) = command in extra || super.isCommandAvailable(command)
      override fun seekToNext() = ctl("next")
      override fun seekToNextMediaItem() = ctl("next")
      override fun seekToPrevious() = ctl("prev")
      override fun seekToPreviousMediaItem() = ctl("prev")
    }
    Viz.sink = { b -> emit(JSObject().put("type", "viz").put("b", org.json.JSONArray(b.map { it.toInt() }))) }
    ready = true
    return this
  }

  private fun ctl(action: String) = emit(JSObject().put("type", "ctl").put("action", action))

  private fun emit(o: JSObject) {
    sink?.invoke(o)
  }

  private fun idle() = exo.mediaItemCount == 0 || exo.playbackState == Player.STATE_IDLE || exo.playbackState == Player.STATE_ENDED

  fun push() {
    if (!ready) return
    vizClock() // при загрузке — позиция, с которой трек начнётся
    val dur = exo.duration
    emit(
      JSObject()
        .put("type", "state")
        .put("pos", if (loading) 0.0 else exo.currentPosition / 1000.0)
        .put("duration", if (loading || dur == C.TIME_UNSET) 0.0 else dur / 1000.0)
        .put("paused", !exo.playWhenReady)
        .put("idle", idle())
        .put("file", file)
    )
  }

  private val ticker = object : Runnable {
    override fun run() {
      if (!exo.isPlaying && !loading) return
      push()
      main.postDelayed(this, 250)
    }
  }

  private fun tick() {
    main.removeCallbacks(ticker)
    main.postDelayed(ticker, 250)
  }

  /** Визуализатор (Viz.kt): замеры — только пока он включён и виден. */
  fun viz(on: Boolean) {
    if (ready) vizClock()
    Viz.on = on
  }

  private fun vizClock(pos: Long = exo.currentPosition) = Viz.clock(pos, exo.isPlaying, exo.playbackParameters.speed)

  /** Последнее «что играет» от интерфейса (tray.js → mini_state): название, исполнитель, обложка. */
  private var lastMeta: Triple<String, String, String>? = null

  private fun metadata(title: String, artist: String, cover: String): MediaMetadata {
    val m = MediaMetadata.Builder().setTitle(title)
    if (artist.isNotEmpty()) m.setArtist(artist)
    if (cover.startsWith("https://")) m.setArtworkUri(Uri.parse(cover))
    return m.build()
  }

  fun load(src: String, title: String, ua: String, referer: String, start: Double, pause: Boolean) {
    val local = !src.startsWith("https://")
    val uri = if (local) Uri.fromFile(File(src)) else Uri.parse(src)
    // интерфейс обычно присылает название, исполнителя и обложку раньше, чем трек загрузится: тогда уведомление сразу
    // правильное, без «Исполнитель — Название» одной строкой и пустой обложки (title здесь — как раз эта строка)
    val m = lastMeta?.takeIf { it.first.isNotEmpty() && title.contains(it.first) }
    val md = if (m != null) metadata(m.first, m.second, m.third) else metadata(title, "", "")
    val b = MediaItem.Builder().setUri(uri).setMediaMetadata(md)
    if (!local && (uri.path ?: "").endsWith(".m3u8")) b.setMimeType(MimeTypes.APPLICATION_M3U8)
    headers = ua to referer
    triedHls = false
    open(b.build(), (start * 1000).toLong(), pause)
  }

  private fun open(it: MediaItem, startMs: Long, pause: Boolean) {
    item = it
    val uri = it.localConfiguration!!.uri
    val source: MediaSource = if (uri.scheme == "file") {
      ProgressiveMediaSource.Factory(DefaultDataSource.Factory(app)).createMediaSource(it)
    } else {
      val http = DefaultHttpDataSource.Factory()
        .setAllowCrossProtocolRedirects(false)
        .setConnectTimeoutMs(15_000)
        .setReadTimeoutMs(20_000)
      if (headers.first.isNotEmpty()) http.setUserAgent(headers.first)
      if (headers.second.isNotEmpty()) http.setDefaultRequestProperties(mapOf("Referer" to headers.second))
      if (it.localConfiguration?.mimeType == MimeTypes.APPLICATION_M3U8) HlsMediaSource.Factory(http).createMediaSource(it)
      else ProgressiveMediaSource.Factory(http).createMediaSource(it)
    }
    loading = true
    Viz.clock(startMs.coerceAtLeast(0), false, exo.playbackParameters.speed)
    exo.setMediaSource(source, startMs.coerceAtLeast(0))
    exo.playWhenReady = !pause
    exo.prepare()
    push()
    tick()
  }

  /** Подпись и обложка трека (уведомление, экран блокировки) — без перезагрузки звука. */
  fun meta(title: String, artist: String, cover: String) {
    lastMeta = Triple(title, artist, cover)
    val cur = item ?: return
    val md = metadata(title, artist, cover)
    if (cur.mediaMetadata == md) return
    val next = cur.buildUpon().setMediaMetadata(md).build()
    item = next
    if (exo.mediaItemCount > 0) exo.replaceMediaItem(0, next)
  }

  fun pause(paused: Boolean) {
    exo.playWhenReady = !paused
  }

  fun seek(pos: Double) {
    if (exo.mediaItemCount == 0) return
    val ms = (pos * 1000).toLong().coerceAtLeast(0)
    vizClock(ms)
    exo.seekTo(ms)
  }

  fun stop() {
    loading = false
    item = null
    exo.stop()
    exo.clearMediaItems()
    push()
  }

  fun volume(gain: Float) {
    exo.volume = gain.coerceIn(0f, 1f)
  }

  fun speed(rate: Float) {
    exo.setPlaybackSpeed(rate.coerceIn(0.25f, 4f))
  }

  fun effects(eq: Boolean, preDb: Float, g: FloatArray, normalize: Boolean) {
    eqOn = eq
    pre = preDb
    gains = g
    norm = normalize
    applyEffects()
  }

  /** Верхние границы 10 полос (между центрами 32 Гц … 16 кГц, как у эквалайзера на компьютере). */
  private val cutoffs = floatArrayOf(45f, 89f, 177f, 354f, 707f, 1414f, 2828f, 5657f, 11314f, 20000f)

  private fun applyEffects() {
    if (!ready || Build.VERSION.SDK_INT < 28) return
    val sid = exo.audioSessionId
    if (sid == C.AUDIO_SESSION_ID_UNSET) return
    try {
      if (!eqOn && !norm) {
        dp?.enabled = false
        return
      }
      if (dp == null || dpSession != sid) {
        dp?.release()
        val cfg = DynamicsProcessing.Config.Builder(
          DynamicsProcessing.VARIANT_FAVOR_FREQUENCY_RESOLUTION, 2,
          true, cutoffs.size, // эквалайзер
          true, 1, // сжатие — для выравнивания
          false, 0,
          true, // ограничитель: подъём полос и выравнивание не уводят звук за 0 дБ
        ).build()
        dp = DynamicsProcessing(0, sid, cfg)
        dpSession = sid
      }
      val d = dp ?: return
      val eq = DynamicsProcessing.Eq(true, eqOn, cutoffs.size)
      for (i in cutoffs.indices) {
        val band = eq.getBand(i)
        band.isEnabled = eqOn
        band.cutoffFrequency = cutoffs[i]
        band.gain = if (eqOn) gains.getOrElse(i) { 0f } else 0f
      }
      d.setPreEqAllChannelsTo(eq)
      d.setInputGainAllChannelsTo(if (eqOn) pre else 0f)
      // «Постоянный уровень громкости»: тихие места громче, громкие — тише (на лету; замера EBU R128 тут нет)
      val mbc = DynamicsProcessing.Mbc(true, norm, 1)
      mbc.getBand(0).apply {
        isEnabled = norm
        cutoffFrequency = 20000f
        attackTime = 10f
        releaseTime = 400f
        ratio = 3f
        threshold = -24f
        kneeWidth = 6f
        noiseGateThreshold = -90f
        expanderRatio = 1f
        preGain = 0f
        postGain = if (norm) 8f else 0f
      }
      d.setMbcAllChannelsTo(mbc)
      d.setLimiterAllChannelsTo(DynamicsProcessing.Limiter(true, true, 0, 1f, 60f, 10f, -1f, 0f))
      d.enabled = true
    } catch (e: Exception) {
      // эффекты недоступны на этом устройстве — звук идёт как есть
      dp = null
    }
  }
}

/**
 * Фоновая служба плеера (Media3): уведомление с обложкой и кнопками, экран блокировки, гарнитура, часы.
 * Пока играет — на переднем плане (Android её не остановит); приложение смахнули, а музыка стоит — служба выходит.
 */
@UnstableApi
class PlaybackService : MediaSessionService() {
  private var session: MediaSession? = null

  /** «×» в уведомлении и на экране блокировки: остановить трек — уведомление уходит само (плеер пуст). */
  private val close = SessionCommand("vkm.close", Bundle.EMPTY)

  private val callback = object : MediaSession.Callback {
    override fun onConnect(session: MediaSession, controller: MediaSession.ControllerInfo): MediaSession.ConnectionResult {
      val cmds = MediaSession.ConnectionResult.DEFAULT_SESSION_COMMANDS.buildUpon().add(close).build()
      return MediaSession.ConnectionResult.AcceptedResultBuilder(session).setAvailableSessionCommands(cmds).build()
    }

    override fun onCustomCommand(
      session: MediaSession,
      controller: MediaSession.ControllerInfo,
      customCommand: SessionCommand,
      args: Bundle,
    ): ListenableFuture<SessionResult> {
      if (customCommand.customAction == close.customAction) {
        Engine.get(this@PlaybackService).stop()
        return Futures.immediateFuture(SessionResult(SessionResult.RESULT_SUCCESS))
      }
      return super.onCustomCommand(session, controller, customCommand, args)
    }
  }

  override fun onCreate() {
    super.onCreate()
    val open = PendingIntent.getActivity(
      this, 0,
      Intent(this, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
      PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
    )
    // «×» — слева от «назад»
    val closeButton = CommandButton.Builder(CommandButton.ICON_UNDEFINED)
      .setCustomIconResId(R.drawable.ic_vkm_close)
      .setDisplayName(getString(R.string.vkm_close))
      .setSessionCommand(close)
      .setSlots(CommandButton.SLOT_BACK_SECONDARY)
      .build()
    // маленький значок уведомления (на обложке и в строке состояния) — логотип VK Music, а не значок Media3
    setMediaNotificationProvider(DefaultMediaNotificationProvider.Builder(this).build().apply { setSmallIcon(R.drawable.ic_vkm_notification) })
    session = MediaSession.Builder(this, Engine.get(this).session)
      .setSessionActivity(open)
      .setCallback(callback)
      .setMediaButtonPreferences(ImmutableList.of(closeButton))
      .build()
  }

  override fun onGetSession(controllerInfo: MediaSession.ControllerInfo): MediaSession? = session

  override fun onTaskRemoved(rootIntent: Intent?) {
    val p = session?.player
    if (p == null || !p.playWhenReady || p.mediaItemCount == 0) stopSelf()
  }

  override fun onDestroy() {
    session?.release()
    session = null
    super.onDestroy()
  }
}
