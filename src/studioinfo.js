// Сводка о треке в «Студии» (studio.js; как у Whisper Yandex — его studioinfo.js без изменений в расчётах): темп (BPM),
// тональность, громкость — по уже разобранному AudioBuffer (тот же, по которому рисуется волна), без сети и библиотек.
// Это оценка: у живой музыки темп «плавает», а тональность в модальной и атональной музыке определить трудно —
// поэтому рядом показывается, насколько оценка уверенная.
//   window.studioAnalyze(buf) → { bpm, bpmSure, key, keyShort, keySure, peakDb, rmsDb }
(() => {
  // ---------- моно-сигнал из середины трека (до 90 с: дольше — дольше считать, точнее почти не становится) ----------
  function monoSlice(buf, maxSec) {
    const sr = buf.sampleRate, len = buf.length, take = Math.min(len, Math.floor(sr * maxSec));
    const start = Math.max(0, Math.floor((len - take) / 2));
    const chs = Math.min(2, buf.numberOfChannels), out = new Float32Array(take);
    for (let c = 0; c < chs; c++) {
      const d = buf.getChannelData(c);
      for (let i = 0; i < take; i++) out[i] += d[start + i] / chs;
    }
    return out;
  }

  // ---------- темп ----------
  // Огибающая «атак»: рост энергии баса (одно-полюсный фильтр ~150 Гц — бочка) и всего сигнала, шаг ~5,8 мс.
  // Дальше — автокорреляция огибающей: период, через который атаки повторяются, и есть доля такта.
  function tempo(x, sr) {
    const hop = Math.max(64, Math.round(sr / 172));
    const fr = sr / hop, n = Math.floor(x.length / hop);
    if (n < fr * 8) return null;                         // меньше 8 секунд — темп не понять
    const a = Math.exp((-2 * Math.PI * 150) / sr);
    let lp = 0, prevL = 0, prevA = 0;
    const on = new Float32Array(n);
    for (let k = 0; k < n; k++) {
      let el = 0, ea = 0;
      for (let i = k * hop, e = i + hop; i < e; i++) {
        const v = x[i];
        lp = a * lp + (1 - a) * v;
        el += lp * lp; ea += v * v;
      }
      const L = Math.log1p(el * 1e3), A = Math.log1p(ea * 1e2);
      on[k] = 0.65 * Math.max(0, L - prevL) + 0.35 * Math.max(0, A - prevA);
      prevL = L; prevA = A;
    }
    // убрать медленные изменения громкости: из каждой точки — среднее за ~0,4 с вокруг
    const w = Math.round(fr * 0.2), o = new Float32Array(n);
    let sum = 0;
    for (let k = 0; k < Math.min(n, w); k++) sum += on[k];
    for (let k = 0; k < n; k++) {
      if (k + w < n) sum += on[k + w];
      if (k - w - 1 >= 0) sum -= on[k - w - 1];
      const cnt = Math.min(n - 1, k + w) - Math.max(0, k - w) + 1;
      o[k] = Math.max(0, on[k] - sum / cnt);
    }
    // сгладить (окно ~30 мс): удары попадают то в один шаг огибающей, то в соседний — без сглаживания
    // автокорреляция «промахивается» на дробных периодах
    const sm = new Float32Array(n), K = [0.06, 0.24, 0.4, 0.24, 0.06];
    for (let k = 0; k < n; k++) { let v = 0; for (let j = -2; j <= 2; j++) { const q = k + j; if (q >= 0 && q < n) v += o[q] * K[j + 2]; } sm[k] = v; }
    o.set(sm);
    const ac = (lag) => {                                // автокорреляция с дробным сдвигом
      const l0 = Math.floor(lag), f = lag - l0;
      let s = 0;
      for (let k = 0; k + l0 + 1 < n; k++) s += o[k] * (o[k + l0] * (1 - f) + o[k + l0 + 1] * f);
      return s / (n - l0);
    };
    // счёт темпа: сама доля и её кратные (такт из 2 и 4 долей), с лёгким предпочтением привычных 90–150 BPM
    const score = (bpm) => {
      const lag = (60 * fr) / bpm;
      const s = ac(lag) + 0.5 * ac(lag * 2) + 0.25 * ac(lag * 4);
      const g = Math.log2(bpm / 120);
      return s * Math.exp(-0.5 * (g / 0.9) ** 2);
    };
    let best = 0, bs = -1;
    const all = [];
    for (let b = 60; b <= 200; b += 0.5) { const s = score(b); all.push(s); if (s > bs) { bs = s; best = b; } }
    for (let b = best - 0.5; b <= best + 0.5; b += 0.05) { const s = score(b); if (s > bs) { bs = s; best = b; } }
    // уверенность: насколько лучший темп выделяется на фоне остальных
    const mean = all.reduce((p, v) => p + v, 0) / all.length;
    const sure = mean > 0 ? Math.max(0, Math.min(1, (bs / mean - 1.15) / 0.7)) : 0;
    return { bpm: Math.round(best * 10) / 10, sure };
  }

  // ---------- тональность ----------
  // Хрома (сколько энергии у каждой из 12 нот) по спектру ~150 окон через весь трек, затем сравнение с профилями
  // мажора и минора Крумхансла — Кесслер (24 тональности, лучшая корреляция).
  function fft(re, im) {                                  // на месте, длина — степень двойки
    const n = re.length;
    for (let i = 1, j = 0; i < n; i++) {
      let bit = n >> 1;
      for (; j & bit; bit >>= 1) j ^= bit;
      j ^= bit;
      if (i < j) { [re[i], re[j]] = [re[j], re[i]]; [im[i], im[j]] = [im[j], im[i]]; }
    }
    for (let len = 2; len <= n; len <<= 1) {
      const ang = (-2 * Math.PI) / len, wr = Math.cos(ang), wi = Math.sin(ang);
      for (let i = 0; i < n; i += len) {
        let cr = 1, ci = 0;
        for (let k = 0; k < len / 2; k++) {
          const a = i + k, b = a + len / 2;
          const tr = re[b] * cr - im[b] * ci, ti = re[b] * ci + im[b] * cr;
          re[b] = re[a] - tr; im[b] = im[a] - ti; re[a] += tr; im[a] += ti;
          const t = cr * wr - ci * wi; ci = cr * wi + ci * wr; cr = t;
        }
      }
    }
  }
  const MAJOR = [6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88];
  const MINOR = [6.33, 2.68, 3.52, 5.38, 2.6, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17];
  const NAMES = ["до", "до-диез", "ре", "ми-бемоль", "ми", "фа", "фа-диез", "соль", "ля-бемоль", "ля", "си-бемоль", "си"];
  const SHORT = ["C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B"];
  function corr(a, b) {
    const n = a.length, ma = a.reduce((p, v) => p + v, 0) / n, mb = b.reduce((p, v) => p + v, 0) / n;
    let s = 0, sa = 0, sb = 0;
    for (let i = 0; i < n; i++) { const x = a[i] - ma, y = b[i] - mb; s += x * y; sa += x * x; sb += y * y; }
    return sa && sb ? s / Math.sqrt(sa * sb) : 0;
  }
  function keyOf(buf) {
    const sr = buf.sampleRate, chs = Math.min(2, buf.numberOfChannels), len = buf.length;
    const dec = Math.max(1, Math.round(sr / 11025)), fs = sr / dec, N = 4096;
    const span = N * dec;
    if (len < span * 4) return null;
    const frames = Math.min(150, Math.floor(len / span));
    const step = Math.floor((len - span) / frames);
    const chroma = new Float64Array(12), re = new Float64Array(N), im = new Float64Array(N);
    const data = [...Array(chs)].map((_, c) => buf.getChannelData(c));
    const lo = Math.ceil((65 * N) / fs), hi = Math.min(N / 2 - 1, Math.floor((2100 * N) / fs));
    const pcOf = new Int8Array(N / 2).fill(-1);
    for (let k = lo; k <= hi; k++) { const f = (k * fs) / N; pcOf[k] = ((Math.round(12 * Math.log2(f / 440) + 69) % 12) + 12) % 12; }
    for (let fi = 0; fi < frames; fi++) {
      const at = fi * step;
      for (let i = 0; i < N; i++) {
        let v = 0;
        const base = at + i * dec;
        for (let c = 0; c < chs; c++) for (let d = 0; d < dec; d++) v += data[c][base + d];
        const w = 0.5 - 0.5 * Math.cos((2 * Math.PI * i) / (N - 1));
        re[i] = (v / (chs * dec)) * w; im[i] = 0;
      }
      fft(re, im);
      for (let k = lo; k <= hi; k++) chroma[pcOf[k]] += Math.sqrt(re[k] * re[k] + im[k] * im[k]);
    }
    const ch = [...chroma];
    let best = null, second = -2;
    for (let t = 0; t < 12; t++) {
      for (const [prof, minor] of [[MAJOR, false], [MINOR, true]]) {
        const rot = prof.map((_, i) => prof[(i - t + 12) % 12]);
        const r = corr(ch, rot);
        if (!best || r > best.r) { if (best) second = Math.max(second, best.r); best = { r, t, minor }; }
        else second = Math.max(second, r);
      }
    }
    if (!best) return null;
    const sure = Math.max(0, Math.min(1, (best.r - 0.45) / 0.4)) * Math.min(1, (best.r - second) * 8 + 0.4);
    return {
      key: `${NAMES[best.t]} ${best.minor ? "минор" : "мажор"}`,
      keyShort: SHORT[best.t] + (best.minor ? "m" : ""),
      sure,
    };
  }

  // ---------- громкость ----------
  function loudness(buf) {
    const chs = buf.numberOfChannels, len = buf.length;
    let peak = 0, sq = 0, n = 0;
    for (let c = 0; c < chs; c++) {
      const d = buf.getChannelData(c);
      for (let i = 0; i < len; i += 2) { const v = d[i] < 0 ? -d[i] : d[i]; if (v > peak) peak = v; sq += v * v; n++; }
    }
    const db = (v) => (v > 0 ? 20 * Math.log10(v) : -Infinity);
    return { peakDb: db(peak), rmsDb: db(Math.sqrt(sq / Math.max(1, n))) };
  }

  window.studioAnalyze = (buf) => {
    const out = { bpm: null, bpmSure: 0, key: null, keyShort: null, keySure: 0, peakDb: null, rmsDb: null };
    try { Object.assign(out, loudness(buf)); } catch {}
    try { const t = tempo(monoSlice(buf, 90), buf.sampleRate); if (t) { out.bpm = t.bpm; out.bpmSure = t.sure; } } catch {}
    try { const k = keyOf(buf); if (k) { out.key = k.key; out.keyShort = k.keyShort; out.keySure = k.sure; } } catch {}
    return out;
  };
})();
