package io.github.whispermusic.vkmusic

import android.os.SystemClock
import androidx.media3.common.C
import androidx.media3.common.util.UnstableApi
import androidx.media3.exoplayer.audio.TeeAudioProcessor
import java.nio.ByteBuffer
import java.nio.ByteOrder
import kotlin.math.PI
import kotlin.math.ceil
import kotlin.math.cos
import kotlin.math.floor
import kotlin.math.log10
import kotlin.math.max
import kotlin.math.pow
import kotlin.math.roundToInt
import kotlin.math.sin
import kotlin.math.sqrt

/**
 * Визуализатор VK Микса на телефоне (viz.js): ответвитель звука ExoPlayer (`TeeAudioProcessor` в `DefaultAudioSink`,
 * до изменения скорости — отсчёты во времени трека) → моно → окно Ханна 2048 → БПФ → 24 полосы 40 Гц … 12 кГц
 * (как ветка анализа mpv на компьютере, src-tauri/src/viz.rs) — RMS полосы в дБ, ~32 замера в секунду.
 *
 * Замеры считаются в потоке воспроизведения раньше, чем звук слышен (на размер буфера AudioTrack), поэтому у каждого —
 * время в треке (`baseMs` с последнего сброса + отсчёты), а отдаёт их свой поток `viz-emit` (~30 раз в секунду), когда
 * позиция до него дошла. Позицию Engine сообщает из главного потока (`clock`: при каждом состоянии, перемотке, загрузке),
 * между сообщениями она досчитывается по часам — отдача из главного потока, занятого WebView, шла лишь ~15 раз в секунду.
 * Сброс (`flush`) бывает при новом треке и перемотке — его время — текущая позиция часов.
 */
@UnstableApi
object Viz : TeeAudioProcessor.AudioBufferSink {
  const val BANDS = 24
  private const val LO = 40.0
  private const val HI = 12000.0
  private const val N = 2048

  /** включён и виден (команда viz из Rust) */
  @Volatile var on = false
  /** события в Rust (Engine.sink) */
  @Volatile var sink: ((ShortArray) -> Unit)? = null

  // часы: позиция, мс, в момент `clockAt` (SystemClock.elapsedRealtime), играет ли и с какой скоростью
  @Volatile private var clockPos = 0L
  @Volatile private var clockAt = 0L
  @Volatile private var playing = false
  @Volatile private var speed = 1f

  fun clock(pos: Long, isPlaying: Boolean, rate: Float) {
    clockPos = pos
    clockAt = SystemClock.elapsedRealtime()
    playing = isPlaying
    speed = rate
  }

  private fun now(): Long {
    if (!playing) return clockPos
    return clockPos + ((SystemClock.elapsedRealtime() - clockAt).coerceIn(0, 1000) * speed).toLong()
  }

  init {
    Thread({
      while (true) {
        try { Thread.sleep(33) } catch (_: InterruptedException) {}
        if (!on || !playing) continue
        val b = take(now()) ?: continue
        sink?.invoke(b)
      }
    }, "viz-emit").apply { isDaemon = true }.start()
  }

  private var sr = 44100
  private var ch = 2
  private var enc = C.ENCODING_PCM_16BIT
  private var hop = 1378
  private var baseMs = 0L
  private var samples = 0L

  private val ring = FloatArray(N)
  private var w = 0
  private var filled = 0
  private var since = 0

  private val win = FloatArray(N) { i -> (0.5 - 0.5 * cos(2 * PI * i / N)).toFloat() }
  private val winPow = win.sumOf { (it * it).toDouble() }
  private val re = DoubleArray(N)
  private val im = DoubleArray(N)
  private val rev = IntArray(N).also { r ->
    val bits = Integer.numberOfTrailingZeros(N)
    for (i in 0 until N) r[i] = Integer.reverse(i) ushr (32 - bits)
  }
  private val cosT = DoubleArray(N / 2) { cos(2 * PI * it / N) }
  private val sinT = DoubleArray(N / 2) { -sin(2 * PI * it / N) }
  private var kLo = IntArray(BANDS)
  private var kHi = IntArray(BANDS)
  private var edgesFor = 0

  /** замеры, ещё не дошедшие до позиции: (время, мс; уровни, дБ) */
  private val queue = ArrayDeque<Pair<Long, ShortArray>>()

  override fun flush(sampleRateHz: Int, channelCount: Int, encoding: Int) {
    sr = sampleRateHz.coerceAtLeast(8000)
    ch = channelCount.coerceAtLeast(1)
    enc = encoding
    hop = max(256, sr / 32)
    baseMs = now()
    samples = 0
    filled = 0
    since = 0
    synchronized(queue) { queue.clear() }
  }

  override fun handleBuffer(buffer: ByteBuffer) {
    val b = buffer.duplicate().order(ByteOrder.nativeOrder())
    val bytes = when (enc) {
      C.ENCODING_PCM_16BIT -> 2
      C.ENCODING_PCM_FLOAT -> 4
      else -> { samples += b.remaining() / (4 * ch); return }   // других форматов у декодеров ExoPlayer почти не бывает
    }
    val frames = b.remaining() / (bytes * ch)
    if (!on) { samples += frames; return }
    for (f in 0 until frames) {
      var s = 0f
      for (c in 0 until ch) s += if (bytes == 2) b.short / 32768f else b.float
      ring[w] = s / ch
      w = (w + 1) % N
      if (filled < N) filled++
      samples++
      if (++since >= hop && filled == N) {
        since = 0
        analyze(baseMs + (samples - N / 2) * 1000 / sr)
      }
    }
  }

  private fun edges() {
    if (edgesFor == sr) return
    edgesFor = sr
    val r = (HI / LO).pow(1.0 / (BANDS - 1))
    val df = sr.toDouble() / N
    for (i in 0 until BANDS) {
      val c = LO * r.pow(i)
      var a = ceil(c / sqrt(r) / df).toInt()
      var z = floor(c * sqrt(r) / df).toInt()
      if (z < a) { a = (c / df).roundToInt().coerceAtLeast(1); z = a }
      kLo[i] = a.coerceIn(1, N / 2 - 1)
      kHi[i] = z.coerceIn(kLo[i], N / 2 - 1)
    }
  }

  private fun analyze(atMs: Long) {
    edges()
    for (i in 0 until N) {
      val j = rev[i]
      re[j] = (ring[(w + i) % N] * win[i]).toDouble()
      im[j] = 0.0
    }
    var size = 2
    while (size <= N) {
      val half = size / 2
      val step = N / size
      var s = 0
      while (s < N) {
        for (k in 0 until half) {
          val tr = cosT[k * step]
          val ti = sinT[k * step]
          val a = s + k
          val bb = a + half
          val xr = re[bb] * tr - im[bb] * ti
          val xi = re[bb] * ti + im[bb] * tr
          re[bb] = re[a] - xr; im[bb] = im[a] - xi
          re[a] += xr; im[a] += xi
        }
        s += size
      }
      size *= 2
    }
    // средний квадрат полосы: 2·Σ|X|² / (N·Σw²) — односторонний спектр, поправка на окно
    val out = ShortArray(BANDS)
    for (i in 0 until BANDS) {
      var p = 0.0
      for (k in kLo[i]..kHi[i]) p += re[k] * re[k] + im[k] * im[k]
      val ms = 2 * p / (N * winPow)
      val db = if (ms > 1e-10) 10 * log10(ms) else -100.0
      out[i] = db.coerceIn(-100.0, 0.0).roundToInt().toShort()
    }
    synchronized(queue) {
      queue.addLast(atMs to out)
      while (queue.size > 300) queue.removeFirst()
    }
  }

  /** Последний замер, время которого уже наступило. */
  private fun take(pos: Long): ShortArray? {
    var last: ShortArray? = null
    synchronized(queue) {
      while (queue.isNotEmpty() && queue.first().first <= pos) last = queue.removeFirst().second
    }
    return last
  }
}
