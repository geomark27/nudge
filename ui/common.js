// Utilidades compartidas por la ventana principal y la de alertas.
// Tauri inyecta `window.__TAURI__` gracias a `withGlobalTauri`.
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const DAY_LABELS = ["L", "M", "X", "J", "V", "S", "D"]; // lunes = bit 0
const ALL_DAYS = 0b1111111;
const WEEKDAYS = 0b0011111;

/** "light"/"dark" fuerzan el tema; "system" deja que mande `prefers-color-scheme`. */
function applyTheme(theme) {
  if (theme === "light" || theme === "dark") document.documentElement.dataset.theme = theme;
  else delete document.documentElement.dataset.theme;
}

// Todas las ventanas siguen el tema guardado y sus cambios.
const themeReady = invoke("get_theme").then((theme) => {
  applyTheme(theme);
  return theme;
});
listen("theme-changed", (event) => applyTheme(event.payload));

/** Crea un elemento; los textos siempre van por textContent. */
function el(tag, props = {}, ...children) {
  const node = document.createElement(tag);
  for (const [key, value] of Object.entries(props)) {
    if (key === "class") node.className = value;
    else if (key === "text") node.textContent = value;
    else if (key.startsWith("on")) node.addEventListener(key.slice(2), value);
    else node.setAttribute(key, value);
  }
  node.append(...children.filter((c) => c != null));
  return node;
}

function fmtTime(iso) {
  return new Date(iso).toLocaleTimeString("es-EC", { hour: "2-digit", minute: "2-digit" });
}

/** "14:00", "mañana 08:00" o "lun 08:00". */
function fmtWhen(iso) {
  const date = new Date(iso);
  const today = new Date();
  const tomorrow = new Date(today);
  tomorrow.setDate(today.getDate() + 1);
  const same = (a, b) => a.toDateString() === b.toDateString();
  if (same(date, today)) return fmtTime(iso);
  if (same(date, tomorrow)) return `mañana ${fmtTime(iso)}`;
  return `${date.toLocaleDateString("es-EC", { weekday: "short", day: "numeric" })} ${fmtTime(iso)}`;
}

function fmtDuration(min) {
  if (min < 60) return `${min} min`;
  const h = Math.floor(min / 60);
  const m = min % 60;
  return m ? `${h} h ${m} min` : `${h} h`;
}

function fmtDays(mask) {
  if ((mask & ALL_DAYS) === ALL_DAYS) return "todos los días";
  if (mask === WEEKDAYS) return "lunes a viernes";
  if (mask === 0b1100000) return "fines de semana";
  return DAY_LABELS.filter((_, i) => mask & (1 << i)).join(" ");
}

function describeSchedule(r) {
  const s = r.schedule;
  // "HH:MM" se compara bien como texto; fin < inicio = cruza la medianoche.
  const overnight = r.window.end < r.window.start ? " (al día siguiente)" : "";
  const range = `${r.window.start}–${r.window.end}${overnight}`;
  let text;
  if (s.kind === "times") text = `A las ${s.times.join(", ")}`;
  else if (s.anchor === "from_done") text = `Cada ${fmtDuration(s.every_min)} desde que lo marcas hecho · ${range}`;
  else text = `Cada ${fmtDuration(s.every_min)} · ${range}`;
  return `${text} · ${fmtDays(r.days)}`;
}

// ---------- Sonidos ----------

const FILE_TONE = "file:";
const AUDIO_TYPES = { mp3: "audio/mpeg", wav: "audio/wav", ogg: "audio/ogg", m4a: "audio/mp4" };

/**
 * Tonos integrados, sintetizados con Web Audio (no necesitan archivos).
 * Cada nota: [frecuencia Hz, inicio s, duración s]. Las claves deben coincidir con
 * `BUILTIN_TONES` en `crates/nudge-core/src/sound.rs`.
 */
const TONES = {
  chime: { label: "Campanita", wave: "sine", gain: 0.3, notes: [[880, 0, 0.6], [1318.5, 0.18, 0.6]] },
  soft: { label: "Suave", wave: "sine", gain: 0.18, notes: [[523.3, 0, 0.9], [659.3, 0.25, 1.1]] },
  double: { label: "Doble toque", wave: "triangle", gain: 0.3, notes: [[987.8, 0, 0.18], [987.8, 0.24, 0.18]] },
  drop: { label: "Gota", wave: "sine", gain: 0.35, notes: [[1567.98, 0, 0.12], [1046.5, 0.06, 0.25]] },
  alarm: {
    label: "Alarma",
    wave: "square",
    gain: 0.12,
    notes: [0, 0.3, 0.6, 0.9].flatMap((t) => [[1046.5, t, 0.12], [784, t + 0.14, 0.12]]),
  },
};

/** Nombre visible de un tono: integrado o archivo propio. */
function toneLabel(tone) {
  if (tone.startsWith(FILE_TONE)) return tone.slice(FILE_TONE.length);
  return TONES[tone]?.label ?? tone;
}

/** Reproduce un tono; si es un archivo que ya no existe, suena la campanita. */
async function playTone(tone = "chime") {
  if (tone.startsWith(FILE_TONE)) {
    try {
      await playFile(tone.slice(FILE_TONE.length));
      return;
    } catch (err) {
      console.warn("no se pudo reproducir el sonido propio; uso la campanita", err);
      tone = "chime";
    }
  }
  synth(TONES[tone] ?? TONES.chime);
}

async function playFile(name) {
  const bytes = await invoke("read_sound", { name });
  const type = AUDIO_TYPES[name.split(".").pop().toLowerCase()] ?? "";
  const url = URL.createObjectURL(new Blob([bytes], { type }));
  const audio = new Audio(url);
  audio.addEventListener("ended", () => URL.revokeObjectURL(url), { once: true });
  try {
    await audio.play();
  } catch (err) {
    URL.revokeObjectURL(url);
    throw err;
  }
}

function synth({ wave, gain: peak, notes }) {
  try {
    const ctx = new AudioContext();
    let end = 0;
    for (const [freq, offset, duration] of notes) {
      const osc = ctx.createOscillator();
      const gain = ctx.createGain();
      const start = ctx.currentTime + offset;
      osc.type = wave;
      osc.frequency.value = freq;
      gain.gain.setValueAtTime(0.0001, start);
      gain.gain.exponentialRampToValueAtTime(peak, start + 0.02);
      gain.gain.exponentialRampToValueAtTime(0.0001, start + duration);
      osc.connect(gain).connect(ctx.destination);
      osc.start(start);
      osc.stop(start + duration + 0.05);
      end = Math.max(end, offset + duration);
    }
    setTimeout(() => ctx.close(), (end + 0.5) * 1000);
  } catch (err) {
    console.warn("no se pudo reproducir el sonido", err);
  }
}
