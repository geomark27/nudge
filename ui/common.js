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
  const range = `${r.window.start}–${r.window.end}`;
  let text;
  if (s.kind === "times") text = `A las ${s.times.join(", ")}`;
  else if (s.anchor === "from_done") text = `Cada ${fmtDuration(s.every_min)} desde que lo marcas hecho · ${range}`;
  else text = `Cada ${fmtDuration(s.every_min)} · ${range}`;
  return `${text} · ${fmtDays(r.days)}`;
}

/** Campanita breve con Web Audio; no necesita archivos de sonido. */
function chime() {
  try {
    const ctx = new AudioContext();
    const notes = [880, 1318.5];
    notes.forEach((freq, i) => {
      const osc = ctx.createOscillator();
      const gain = ctx.createGain();
      const start = ctx.currentTime + i * 0.18;
      osc.type = "sine";
      osc.frequency.value = freq;
      gain.gain.setValueAtTime(0.0001, start);
      gain.gain.exponentialRampToValueAtTime(0.3, start + 0.02);
      gain.gain.exponentialRampToValueAtTime(0.0001, start + 0.6);
      osc.connect(gain).connect(ctx.destination);
      osc.start(start);
      osc.stop(start + 0.65);
    });
    setTimeout(() => ctx.close(), 1500);
  } catch (err) {
    console.warn("no se pudo reproducir el sonido", err);
  }
}
