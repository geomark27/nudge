// Ventana de alerta: muestra los recordatorios sin responder, siempre encima.
// Rust la cierra cuando ya no queda ninguno pendiente.

const container = document.getElementById("alerts");

async function refresh({ playSound = false } = {}) {
  const alerts = await invoke("pending_alerts");
  container.replaceChildren(...alerts.map(alertCard));
  const loud = playSound && alerts.find((a) => a.sound);
  if (loud) playTone(loud.tone);
  container.querySelector("button.primary")?.focus();
}

function alertCard(a) {
  const respond = (action) => async () => {
    try {
      await invoke("act", { id: a.reminder_id, action });
    } catch (err) {
      alert(err);
    }
    await refresh();
  };
  return el("div", { class: `alert-card${a.late ? " late" : ""}` },
    el("h2", { text: a.title }),
    a.message ? el("p", { text: a.message }) : null,
    el("div", { class: "when", text: a.late ? `Pendiente desde las ${fmtTime(a.due_since)}` : "Ahora" }),
    el("div", { class: "buttons" },
      el("button", { class: "primary", text: "Hecho", onclick: respond({ type: "done" }) }),
      el("button", { text: "+5 min", onclick: respond({ type: "snooze", minutes: 5 }) }),
      el("button", { text: "+10 min", onclick: respond({ type: "snooze", minutes: 10 }) }),
      el("button", { class: "ghost", text: "Descartar", onclick: respond({ type: "dismiss" }) })),
  );
}

listen("alerts-changed", () => refresh({ playSound: true }));
// Si se borra o reemplaza un recordatorio pendiente, quitarlo sin volver a sonar.
listen("reminders-changed", () => refresh());
refresh({ playSound: true });
