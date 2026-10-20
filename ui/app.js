// Ventana principal: lista, edición, pausa, cumplimiento, historial, sonidos y respaldo.

const $ = (id) => document.getElementById(id);
const ACTION_LABELS = { done: "Hecho", snoozed: "Pospuesto", dismissed: "Descartado" };

let editingId = null;
/** Tono del recordatorio en edición; se conserva aunque se elija "Sin sonido". */
let editingTone = "chime";
let reminders = [];
let sounds = [];

async function refresh() {
  const [list, status, history, stats, soundList] = await Promise.all([
    invoke("list_reminders"),
    invoke("status"),
    invoke("history", { limit: 20 }),
    invoke("stats", { days: Number($("stats-days").value) }),
    invoke("list_sounds"),
  ]);
  reminders = list;
  sounds = soundList;
  renderStatus(status);
  renderReminders(reminders);
  renderStats(stats);
  renderHistory(history);
  renderSounds();
}

function plural(n, one, many) {
  return `${n} ${n === 1 ? one : many}`;
}

/** Aviso breve en la esquina; el texto va por textContent. */
function toast(text) {
  const box = $("toast");
  box.textContent = text;
  box.classList.remove("hidden");
  clearTimeout(toast.timer);
  toast.timer = setTimeout(() => box.classList.add("hidden"), 4000);
}

function renderStatus({ paused_until, autostart, version }) {
  $("version").textContent = `Nudge ${version}`;
  const pill = $("status");
  pill.textContent = paused_until ? `Pausado hasta ${fmtTime(paused_until)}` : "Activo";
  pill.classList.toggle("paused", Boolean(paused_until));
  $("resume").classList.toggle("hidden", !paused_until);
  $("autostart").checked = autostart;
}

function renderReminders(reminders) {
  const list = $("reminders");
  if (!reminders.length) {
    list.replaceChildren(
      el("div", { class: "empty" },
        el("p", { text: "Aún no tienes recordatorios." }),
        el("button", { class: "primary", text: "Crear el primero", onclick: () => openEditor() })),
    );
    return;
  }
  list.replaceChildren(...reminders.map(reminderCard));
}

function reminderCard(r) {
  const toggle = el("input", { type: "checkbox", "aria-label": "Activo" });
  toggle.checked = r.enabled;
  toggle.addEventListener("change", () => run(invoke("set_enabled", { id: r.id, enabled: toggle.checked })));

  let next = null;
  if (r.pending_since) next = el("div", { class: "next pending", text: `Pendiente desde ${fmtTime(r.pending_since)}` });
  else if (r.enabled && r.next_fire_at) next = el("div", { class: "next muted", text: `Próximo: ${fmtWhen(r.next_fire_at)}` });

  return el("div", { class: `card${r.enabled ? "" : " off"}` },
    el("label", { class: "switch" }, toggle, el("span")),
    el("div", {},
      el("h3", { text: r.title }),
      el("div", { class: "meta", text: describeSchedule(r) }),
      next),
    el("div", { class: "actions" },
      el("button", { class: "ghost", text: "Editar", onclick: () => openEditor(r) }),
      el("button", { class: "ghost danger", text: "Eliminar", onclick: () => remove(r) })),
  );
}

function renderStats(items) {
  const list = $("stats");
  if (!items.length) {
    list.replaceChildren(el("div", { class: "empty", text: "Crea un recordatorio para ver tu cumplimiento." }));
    return;
  }
  list.replaceChildren(...items.map((s) => {
    const pct = s.compliance_pct;
    const level = pct == null ? "" : pct >= 80 ? "good" : pct >= 50 ? "fair" : "low";
    const fill = el("span");
    fill.style.width = `${pct ?? 0}%`;
    return el("div", { class: "stat" },
      el("div", { class: "stat-head" },
        el("strong", { text: s.title }),
        el("span", { class: `stat-pct ${level}`, text: pct == null ? "sin respuestas" : `${pct}%` })),
      el("div", { class: `bar ${level}`, role: "img", "aria-label": `${pct ?? 0}% cumplido` }, fill),
      el("div", { class: "meta", text: [
        plural(s.done, "hecha", "hechas"),
        plural(s.snoozed, "pospuesta", "pospuestas"),
        plural(s.dismissed, "descartada", "descartadas"),
      ].join(" · ") }));
  }));
}

function renderSounds() {
  const list = $("sounds");
  if (!sounds.length) {
    list.replaceChildren(el("div", { class: "empty", text: "Aún no agregas sonidos propios." }));
    return;
  }
  list.replaceChildren(...sounds.map((name) =>
    el("div", { class: "card sound" },
      el("div", {}, el("h3", { text: name })),
      el("div", { class: "actions" },
        el("button", { class: "ghost", text: "▶ Probar", onclick: () => playTone(FILE_TONE + name) }),
        el("button", { class: "ghost danger", text: "Eliminar", onclick: () => removeSound(name) })))));
}

function renderHistory(items) {
  const body = $("history");
  if (!items.length) {
    body.replaceChildren(el("tr", {}, el("td", { class: "muted", text: "Sin actividad todavía." })));
    return;
  }
  body.replaceChildren(...items.map((o) =>
    el("tr", {},
      el("td", { text: o.title }),
      el("td", {}, el("span", { class: `tag ${o.action}`, text: ACTION_LABELS[o.action] ?? o.action })),
      el("td", { text: fmtWhen(o.acted_at) }))));
}

async function remove(r) {
  if (!confirm(`¿Eliminar "${r.title}" y su historial?`)) return;
  await run(invoke("delete_reminder", { id: r.id }));
}

/** Ejecuta una llamada y refresca; muestra el error si falla. */
async function run(promise) {
  try {
    await promise;
  } catch (err) {
    alert(err);
  }
  await refresh();
}

// ---------- Editor ----------

function buildDays() {
  $("days").replaceChildren(...DAY_LABELS.map((label, i) =>
    el("label", { title: label }, el("input", { type: "checkbox", value: String(i) }), label)));
}

function setMode(mode) {
  document.querySelector(`input[name=mode][value=${mode}]`).checked = true;
  $("interval-fields").classList.toggle("hidden", mode === "times");
  $("times-field").classList.toggle("hidden", mode !== "times");
  $("window-fields").classList.toggle("hidden", mode === "times");
}

function updateWindowHint() {
  const start = $("start").value;
  const end = $("end").value;
  $("window-hint").classList.toggle("hidden", !(start && end && end < start));
}

/** Llena el selector de sonido; un archivo propio que ya no existe se muestra igual. */
function buildToneOptions(selected) {
  const options = [el("option", { value: "none", text: "Sin sonido" })];
  options.push(...Object.entries(TONES).map(([value, t]) => el("option", { value, text: t.label })));
  const files = sounds.map((name) => FILE_TONE + name);
  if (selected.startsWith(FILE_TONE) && !files.includes(selected)) files.push(selected);
  if (files.length) {
    options.push(el("optgroup", { label: "Mis sonidos" }, ...files.map((value) => {
      const missing = !sounds.includes(value.slice(FILE_TONE.length));
      return el("option", { value, text: missing ? `${toneLabel(value)} (no encontrado)` : toneLabel(value) });
    })));
  }
  $("tone").replaceChildren(...options);
  $("tone").value = selected;
}

function openEditor(r = null) {
  editingId = r?.id ?? null;
  $("form-title").textContent = r ? "Editar recordatorio" : "Nuevo recordatorio";
  $("form-error").textContent = "";
  $("title").value = r?.title ?? "";
  $("message").value = r?.message ?? "";
  $("start").value = r?.window.start ?? "08:00";
  $("end").value = r?.window.end ?? "20:00";
  $("nag").value = String(r?.nag_min ?? 5);
  editingTone = r?.tone ?? "chime";
  buildToneOptions(r && !r.sound ? "none" : editingTone);
  $("enabled").checked = r?.enabled ?? true;
  updateWindowHint();

  const s = r?.schedule;
  const everyMin = s?.kind === "interval" ? s.every_min : 120;
  const inHours = everyMin % 60 === 0;
  $("every").value = String(inHours ? everyMin / 60 : everyMin);
  $("unit").value = inHours ? "60" : "1";
  $("times").value = s?.kind === "times" ? s.times.join(", ") : "";
  setMode(s?.kind === "times" ? "times" : s?.anchor ?? "fixed");

  const mask = r?.days ?? ALL_DAYS;
  document.querySelectorAll("#days input").forEach((input) => {
    input.checked = Boolean(mask & (1 << Number(input.value)));
  });

  $("editor").showModal();
  $("title").focus();
}

function readForm() {
  const mode = document.querySelector("input[name=mode]:checked").value;
  let schedule;
  if (mode === "times") {
    const times = $("times").value.split(",").map((t) => t.trim()).filter(Boolean)
      .map((t) => (/^\d:\d\d$/.test(t) ? `0${t}` : t));
    schedule = { kind: "times", times };
  } else {
    const every = Math.round(Number($("every").value) * Number($("unit").value));
    schedule = { kind: "interval", every_min: every, anchor: mode };
  }
  let days = 0;
  document.querySelectorAll("#days input:checked").forEach((input) => { days |= 1 << Number(input.value); });
  const tone = $("tone").value;
  return {
    title: $("title").value.trim(),
    message: $("message").value.trim(),
    schedule,
    window: { start: $("start").value || "00:00", end: $("end").value || "23:59" },
    days,
    sound: tone !== "none",
    tone: tone === "none" ? editingTone : tone,
    nag_min: Number($("nag").value),
    enabled: $("enabled").checked,
  };
}

async function save(event) {
  event.preventDefault();
  const spec = readForm();
  try {
    if (editingId == null) await invoke("create_reminder", { spec });
    else await invoke("update_reminder", { id: editingId, spec });
    $("editor").close();
    await refresh();
  } catch (err) {
    $("form-error").textContent = String(err);
  }
}

// ---------- Sonidos y respaldo ----------

async function addSound() {
  try {
    const name = await invoke("add_sound");
    if (name) toast(`Sonido "${name}" agregado. Elígelo al editar un recordatorio.`);
  } catch (err) {
    alert(err);
  }
  await refresh();
}

async function removeSound(name) {
  const users = reminders.filter((r) => r.tone === FILE_TONE + name).length;
  const note = users ? `\n\nLo usan ${plural(users, "recordatorio", "recordatorios")}; sonarán con la campanita.` : "";
  if (!confirm(`¿Eliminar el sonido "${name}"?${note}`)) return;
  await run(invoke("remove_sound", { name }));
}

async function exportConfig() {
  try {
    const path = await invoke("export_config");
    if (path) toast(`Respaldo guardado en ${path}`);
  } catch (err) {
    alert(err);
  }
}

async function importConfig(mode) {
  if (mode === "replace" && !confirm(
    "Se borrarán tus recordatorios actuales y su historial, y se reemplazarán por los del archivo. ¿Continuar?",
  )) return;
  try {
    const count = await invoke("import_config", { mode });
    if (count != null) toast(`Importados: ${plural(count, "recordatorio", "recordatorios")}.`);
  } catch (err) {
    alert(err);
  }
  await refresh();
}

// ---------- Actualizaciones ----------

function showUpdate(update) {
  $("update-text").textContent = `Nudge ${update.version} está disponible.`;
  $("update-notes").textContent = update.notes ?? "";
  $("update-notes-box").classList.toggle("hidden", !update.notes);
  $("update-banner").classList.remove("hidden");
}

/** `quiet`: búsqueda automática al abrir; solo muestra algo si hay versión nueva. */
async function checkUpdate({ quiet = false } = {}) {
  if (!quiet) $("update-status").textContent = "Buscando…";
  try {
    const update = await invoke("check_update");
    if (update) showUpdate(update);
    if (!quiet) $("update-status").textContent = update ? "" : "Tienes la última versión.";
  } catch (err) {
    if (!quiet) $("update-status").textContent = String(err);
  }
}

async function installUpdate() {
  const button = $("update-install");
  button.disabled = true;
  button.textContent = "Descargando…";
  try {
    await invoke("install_update"); // si sale bien, Nudge se reinicia ya actualizado
  } catch (err) {
    alert(err);
    button.disabled = false;
    button.textContent = "Actualizar ahora";
  }
}

// ---------- Arranque ----------

buildDays();
document.querySelectorAll("input[name=mode]").forEach((input) =>
  input.addEventListener("change", () => setMode(input.value)));
$("form").addEventListener("submit", save);
$("cancel").addEventListener("click", () => $("editor").close());
$("new").addEventListener("click", () => openEditor());
$("pause-30").addEventListener("click", () => run(invoke("pause", { minutes: 30 })));
$("pause-60").addEventListener("click", () => run(invoke("pause", { minutes: 60 })));
$("resume").addEventListener("click", () => run(invoke("pause", { minutes: null })));
$("autostart").addEventListener("change", (e) => run(invoke("set_autostart", { enabled: e.target.checked })));
$("start").addEventListener("input", updateWindowHint);
$("end").addEventListener("input", updateWindowHint);
$("tone-test").addEventListener("click", () => {
  const tone = $("tone").value;
  if (tone !== "none") playTone(tone);
});
$("stats-days").addEventListener("change", refresh);
$("add-sound").addEventListener("click", addSound);
$("export").addEventListener("click", exportConfig);
$("import-add").addEventListener("click", () => importConfig("add"));
$("import-replace").addEventListener("click", () => importConfig("replace"));
$("theme").addEventListener("change", (e) => invoke("set_theme", { theme: e.target.value }).catch(alert));
themeReady.then((theme) => { $("theme").value = theme; });
listen("theme-changed", (event) => { $("theme").value = event.payload; });

$("update-check").addEventListener("click", () => checkUpdate());
$("update-install").addEventListener("click", installUpdate);
listen("update-available", (event) => showUpdate(event.payload));
listen("update-progress", ({ payload: [done, total] }) => {
  $("update-install").textContent = total
    ? `Descargando ${Math.round((done * 100) / total)}%`
    : `Descargando ${(done / 1048576).toFixed(1)} MB`;
});
checkUpdate({ quiet: true });

listen("reminders-changed", refresh);
// Mantiene al día "Próximo" y el estado de la pausa aunque no haya eventos.
setInterval(refresh, 30_000);
refresh();
