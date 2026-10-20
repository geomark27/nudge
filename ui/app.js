// Ventana principal: lista, edición, pausa e historial.

const $ = (id) => document.getElementById(id);
const ACTION_LABELS = { done: "Hecho", snoozed: "Pospuesto", dismissed: "Descartado" };

let editingId = null;

async function refresh() {
  const [reminders, status, history] = await Promise.all([
    invoke("list_reminders"),
    invoke("status"),
    invoke("history", { limit: 20 }),
  ]);
  renderStatus(status);
  renderReminders(reminders);
  renderHistory(history);
}

function renderStatus({ paused_until, autostart }) {
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

function openEditor(r = null) {
  editingId = r?.id ?? null;
  $("form-title").textContent = r ? "Editar recordatorio" : "Nuevo recordatorio";
  $("form-error").textContent = "";
  $("title").value = r?.title ?? "";
  $("message").value = r?.message ?? "";
  $("start").value = r?.window.start ?? "08:00";
  $("end").value = r?.window.end ?? "20:00";
  $("nag").value = String(r?.nag_min ?? 5);
  $("sound").checked = r?.sound ?? true;
  $("enabled").checked = r?.enabled ?? true;

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
  return {
    title: $("title").value.trim(),
    message: $("message").value.trim(),
    schedule,
    window: { start: $("start").value || "00:00", end: $("end").value || "23:59" },
    days,
    sound: $("sound").checked,
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
$("theme").addEventListener("change", (e) => invoke("set_theme", { theme: e.target.value }).catch(alert));
themeReady.then((theme) => { $("theme").value = theme; });
listen("theme-changed", (event) => { $("theme").value = event.payload; });

listen("reminders-changed", refresh);
// Mantiene al día "Próximo" y el estado de la pausa aunque no haya eventos.
setInterval(refresh, 30_000);
refresh();
