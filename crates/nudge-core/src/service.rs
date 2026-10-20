//! Reglas de negocio: cuándo suena un recordatorio, cuándo insiste y qué pasa al responder.

use chrono::{DateTime, Duration, TimeZone, Utc};

use crate::Error;
use crate::model::{Action, Alert, Backup, ImportMode, Occurrence, Reminder, ReminderSpec, ReminderStats, Theme};
use crate::schedule::next_fire;
use crate::store::Store;

/// A partir de este retraso la alerta se muestra como "pendiente desde…".
const LATE_AFTER: Duration = Duration::minutes(2);
const PAUSED_UNTIL: &str = "paused_until";
const THEME: &str = "theme";

pub struct Service<Tz: TimeZone> {
    store: Store,
    tz: Tz,
}

impl<Tz: TimeZone> Service<Tz> {
    pub fn new(store: Store, tz: Tz) -> Self {
        Self { store, tz }
    }

    pub fn list(&self) -> Result<Vec<Reminder>, Error> {
        self.store.list()
    }

    pub fn create(&self, spec: ReminderSpec, now: DateTime<Utc>) -> Result<Reminder, Error> {
        spec.validate()?;
        let id = self.store.insert(&spec, self.schedule_next(&spec, now), now)?;
        self.store.get(id)
    }

    pub fn update(&self, id: i64, spec: ReminderSpec, now: DateTime<Utc>) -> Result<Reminder, Error> {
        spec.validate()?;
        self.store.update_spec(id, &spec, self.schedule_next(&spec, now), now)?;
        self.store.get(id)
    }

    pub fn set_enabled(&self, id: i64, enabled: bool, now: DateTime<Utc>) -> Result<Reminder, Error> {
        let mut spec = self.store.get(id)?.spec;
        spec.enabled = enabled;
        self.update(id, spec, now)
    }

    pub fn delete(&self, id: i64) -> Result<(), Error> {
        self.store.delete(id)
    }

    /// Revisa qué recordatorios vencieron y devuelve las alertas a mostrar.
    ///
    /// Se llama periódicamente (cada pocos segundos). Como compara contra la hora real y
    /// no depende de temporizadores, tras una suspensión o un apagado cada recordatorio
    /// produce una sola alerta atrasada en lugar de una por cada horario perdido.
    pub fn tick(&self, now: DateTime<Utc>) -> Result<Vec<Alert>, Error> {
        if self.paused_until(now)?.is_some() {
            return Ok(Vec::new());
        }
        let mut alerts = Vec::new();
        for r in self.store.list()? {
            let Some(due) = r.next_fire_at.filter(|t| r.spec.enabled && *t <= now) else {
                continue;
            };
            let pending_since = r.pending_since.unwrap_or(due);
            let nag = (r.spec.nag_min > 0).then(|| now + Duration::minutes(i64::from(r.spec.nag_min)));
            self.store.set_state(r.id, nag, Some(pending_since))?;
            alerts.push(alert(&r, pending_since, now));
        }
        Ok(alerts)
    }

    /// Alertas mostradas que aún no tienen respuesta.
    pub fn pending(&self, now: DateTime<Utc>) -> Result<Vec<Alert>, Error> {
        Ok(self.store.list()?.iter().filter_map(|r| r.pending_since.map(|since| alert(r, since, now))).collect())
    }

    pub fn act(&self, id: i64, action: Action, now: DateTime<Utc>) -> Result<Reminder, Error> {
        let r = self.store.get(id)?;
        let next = match action {
            Action::Snooze { minutes: 0 } => {
                return Err(Error::Validation("posponer requiere al menos 1 minuto".into()));
            }
            Action::Snooze { minutes } => Some(now + Duration::minutes(i64::from(minutes))),
            Action::Done | Action::Dismiss => self.schedule_next(&r.spec, now),
        };
        self.store.add_occurrence(id, r.pending_since.unwrap_or(now), action.as_str(), now)?;
        self.store.set_state(id, next, None)?;
        self.store.get(id)
    }

    /// Pausa todas las alertas hasta `until`; `None` reanuda.
    pub fn pause_until(&self, until: Option<DateTime<Utc>>) -> Result<(), Error> {
        let value = until.map(|t| t.timestamp().to_string());
        self.store.set_setting(PAUSED_UNTIL, value.as_deref())
    }

    /// Fin de la pausa vigente, si la hay.
    pub fn paused_until(&self, now: DateTime<Utc>) -> Result<Option<DateTime<Utc>>, Error> {
        Ok(self
            .store
            .get_setting(PAUSED_UNTIL)?
            .and_then(|v| v.parse().ok())
            .and_then(|secs| DateTime::from_timestamp(secs, 0))
            .filter(|until| *until > now))
    }

    pub fn theme(&self) -> Result<Theme, Error> {
        Ok(self.store.get_setting(THEME)?.map(|v| Theme::parse(&v)).unwrap_or_default())
    }

    /// Guarda el tema; `System` borra el ajuste (es el valor por defecto).
    pub fn set_theme(&self, theme: Theme) -> Result<(), Error> {
        let value = (theme != Theme::System).then(|| theme.as_str());
        self.store.set_setting(THEME, value)
    }

    pub fn history(&self, limit: u32) -> Result<Vec<Occurrence>, Error> {
        self.store.history(limit)
    }

    /// Configuración actual lista para guardarse como respaldo.
    pub fn export(&self, now: DateTime<Utc>) -> Result<Backup, Error> {
        Ok(Backup {
            app: Backup::APP.to_owned(),
            version: Backup::VERSION,
            exported_at: now,
            theme: self.theme()?,
            reminders: self.store.list()?.into_iter().map(|r| r.spec).collect(),
        })
    }

    /// Importa un respaldo y devuelve cuántos recordatorios se crearon. Valida todo antes de
    /// escribir y lo hace en una transacción: o entra el respaldo completo o nada.
    pub fn import(&self, backup: Backup, mode: ImportMode, now: DateTime<Utc>) -> Result<usize, Error> {
        if backup.app != Backup::APP {
            return Err(Error::Validation("el archivo no es un respaldo de Nudge".into()));
        }
        if backup.version > Backup::VERSION {
            return Err(Error::Validation(format!(
                "el respaldo es de una versión más nueva de Nudge (formato {})",
                backup.version
            )));
        }
        for (i, spec) in backup.reminders.iter().enumerate() {
            spec.validate()
                .map_err(|e| Error::Validation(format!("recordatorio {} ({:?}): {e}", i + 1, spec.title)))?;
        }
        self.store.in_transaction(|store| {
            if mode == ImportMode::Replace {
                store.delete_all()?;
            }
            for spec in &backup.reminders {
                store.insert(spec, self.schedule_next(spec, now), now)?;
            }
            self.set_theme(backup.theme)
        })?;
        Ok(backup.reminders.len())
    }

    /// Respuestas por recordatorio desde `since`, incluidos los que aún no tienen ninguna.
    pub fn stats(&self, since: DateTime<Utc>) -> Result<Vec<ReminderStats>, Error> {
        let actions = self.store.actions_since(since)?;
        Ok(self
            .store
            .list()?
            .into_iter()
            .map(|r| {
                let (mut done, mut snoozed, mut dismissed) = (0u32, 0u32, 0u32);
                for (_, action) in actions.iter().filter(|(id, _)| *id == r.id) {
                    match action.as_str() {
                        "done" => done += 1,
                        "snoozed" => snoozed += 1,
                        "dismissed" => dismissed += 1,
                        _ => {}
                    }
                }
                let answered = done + dismissed;
                ReminderStats {
                    reminder_id: r.id,
                    title: r.spec.title,
                    done,
                    snoozed,
                    dismissed,
                    compliance_pct: (answered > 0)
                        .then(|| (f64::from(done) * 100.0 / f64::from(answered)).round() as u8),
                }
            })
            .collect())
    }

    fn schedule_next(&self, spec: &ReminderSpec, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
        if spec.enabled { next_fire(spec, now, &self.tz) } else { None }
    }
}

fn alert(r: &Reminder, due_since: DateTime<Utc>, now: DateTime<Utc>) -> Alert {
    Alert {
        reminder_id: r.id,
        title: r.spec.title.clone(),
        message: r.spec.message.clone(),
        sound: r.spec.sound,
        tone: r.spec.tone.clone(),
        due_since,
        late: now - due_since > LATE_AFTER,
    }
}

#[cfg(test)]
mod tests {
    use chrono::{FixedOffset, NaiveDateTime};

    use super::*;
    use crate::model::{Anchor, Days, Schedule, Window};

    fn ec() -> FixedOffset {
        FixedOffset::west_opt(5 * 3600).unwrap()
    }

    fn at(time: &str) -> DateTime<Utc> {
        let naive = NaiveDateTime::parse_from_str(&format!("2026-10-20 {time}"), "%Y-%m-%d %H:%M").unwrap();
        ec().from_local_datetime(&naive).unwrap().with_timezone(&Utc)
    }

    fn service() -> Service<FixedOffset> {
        Service::new(Store::open_in_memory().unwrap(), ec())
    }

    fn drops(anchor: Anchor) -> ReminderSpec {
        ReminderSpec {
            title: "Gotas para los ojos".into(),
            message: "Una gota en cada ojo".into(),
            schedule: Schedule::Interval { every_min: 120, anchor },
            window: Window { start: "08:00".parse().unwrap(), end: "20:00".parse().unwrap() },
            days: Days::ALL,
            sound: true,
            tone: "chime".into(),
            nag_min: 5,
            enabled: true,
        }
    }

    #[test]
    fn rejects_invalid_spec() {
        let mut spec = drops(Anchor::Fixed);
        spec.title = "  ".into();
        assert!(matches!(service().create(spec, at("09:00")), Err(Error::Validation(_))));
    }

    #[test]
    fn fires_once_when_due_and_nags_until_answered() {
        let svc = service();
        let r = svc.create(drops(Anchor::Fixed), at("09:00")).unwrap();
        assert_eq!(r.next_fire_at, Some(at("10:00")));

        assert!(svc.tick(at("09:59")).unwrap().is_empty());

        let alerts = svc.tick(at("10:00")).unwrap();
        assert_eq!(alerts.len(), 1);
        assert!(!alerts[0].late);
        // Mientras no pase el tiempo de insistencia, no se repite.
        assert!(svc.tick(at("10:03")).unwrap().is_empty());
        // Insiste a los 5 minutos conservando la hora original.
        let again = svc.tick(at("10:05")).unwrap();
        assert_eq!(again.len(), 1);
        assert_eq!(again[0].due_since, at("10:00"));
        assert!(again[0].late);
        assert_eq!(svc.pending(at("10:05")).unwrap().len(), 1);
    }

    #[test]
    fn missed_slots_after_suspend_collapse_into_one_alert() {
        let svc = service();
        svc.create(drops(Anchor::Fixed), at("09:00")).unwrap();
        // El equipo estuvo suspendido de 09:30 a 15:10: se perdieron 10:00, 12:00 y 14:00.
        let alerts = svc.tick(at("15:10")).unwrap();
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].due_since, at("10:00"));
        assert!(alerts[0].late);
        // Al marcarla hecha, sigue el próximo horario, no los perdidos.
        let r = svc.act(alerts[0].reminder_id, Action::Done, at("15:11")).unwrap();
        assert_eq!(r.next_fire_at, Some(at("16:00")));
        assert_eq!(r.pending_since, None);
    }

    #[test]
    fn done_on_from_done_anchor_restarts_the_interval() {
        let svc = service();
        let r = svc.create(drops(Anchor::FromDone), at("08:00")).unwrap();
        assert_eq!(r.next_fire_at, Some(at("10:00")));
        svc.tick(at("10:00")).unwrap();
        let r = svc.act(r.id, Action::Done, at("10:20")).unwrap();
        assert_eq!(r.next_fire_at, Some(at("12:20")));
    }

    #[test]
    fn snooze_hides_alert_and_fires_again_later() {
        let svc = service();
        let r = svc.create(drops(Anchor::Fixed), at("09:00")).unwrap();
        svc.tick(at("10:00")).unwrap();
        svc.act(r.id, Action::Snooze { minutes: 10 }, at("10:01")).unwrap();
        assert!(svc.pending(at("10:02")).unwrap().is_empty());
        assert!(svc.tick(at("10:10")).unwrap().is_empty());
        assert_eq!(svc.tick(at("10:11")).unwrap().len(), 1);

        let history = svc.history(10).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].action, "snoozed");
        assert_eq!(history[0].due_at, at("10:00"));
    }

    #[test]
    fn pause_silences_alerts_until_it_expires() {
        let svc = service();
        svc.create(drops(Anchor::Fixed), at("09:00")).unwrap();
        svc.pause_until(Some(at("11:00"))).unwrap();
        assert!(svc.tick(at("10:00")).unwrap().is_empty());
        assert_eq!(svc.paused_until(at("10:00")).unwrap(), Some(at("11:00")));

        let alerts = svc.tick(at("11:00")).unwrap();
        assert_eq!(alerts.len(), 1);
        assert_eq!(svc.paused_until(at("11:00")).unwrap(), None);
    }

    #[test]
    fn overnight_window_is_valid_but_empty_one_is_not() {
        let svc = service();
        let mut spec = drops(Anchor::Fixed);
        spec.window = Window { start: "22:00".parse().unwrap(), end: "02:00".parse().unwrap() };
        assert_eq!(svc.create(spec.clone(), at("21:00")).unwrap().next_fire_at, Some(at("22:00")));
        spec.window.end = spec.window.start;
        assert!(matches!(svc.create(spec, at("21:00")), Err(Error::Validation(_))));
    }

    #[test]
    fn specs_saved_before_tones_default_to_chime() {
        let old = r#"{"title":"Agua","schedule":{"kind":"times","times":["08:00"]},"days":127,"sound":true}"#;
        let spec: ReminderSpec = serde_json::from_str(old).unwrap();
        assert_eq!(spec.tone, "chime");
        assert!(spec.validate().is_ok());
    }

    #[test]
    fn export_then_import_recreates_the_configuration() {
        let svc = service();
        svc.create(drops(Anchor::Fixed), at("09:00")).unwrap();
        svc.set_theme(Theme::Dark).unwrap();
        let json = serde_json::to_string(&svc.export(at("09:00")).unwrap()).unwrap();

        let other = service();
        let backup: Backup = serde_json::from_str(&json).unwrap();
        assert_eq!(other.import(backup, ImportMode::Add, at("09:30")).unwrap(), 1);
        let list = other.list().unwrap();
        assert_eq!(list[0].spec, drops(Anchor::Fixed));
        // El estado se recalcula al importar, no se copia.
        assert_eq!(list[0].next_fire_at, Some(at("10:00")));
        assert_eq!(other.theme().unwrap(), Theme::Dark);
    }

    #[test]
    fn import_add_keeps_existing_and_replace_removes_them() {
        let svc = service();
        let old = svc.create(drops(Anchor::Fixed), at("09:00")).unwrap();
        svc.tick(at("10:00")).unwrap();
        svc.act(old.id, Action::Done, at("10:01")).unwrap();
        let backup = svc.export(at("10:02")).unwrap();

        svc.import(backup.clone(), ImportMode::Add, at("10:03")).unwrap();
        assert_eq!(svc.list().unwrap().len(), 2);

        svc.import(backup, ImportMode::Replace, at("10:04")).unwrap();
        let list = svc.list().unwrap();
        assert_eq!(list.len(), 1);
        assert!(svc.history(10).unwrap().is_empty());
    }

    #[test]
    fn invalid_backup_changes_nothing() {
        let svc = service();
        svc.create(drops(Anchor::Fixed), at("09:00")).unwrap();
        let mut backup = svc.export(at("09:00")).unwrap();
        let mut bad = drops(Anchor::Fixed);
        bad.title = String::new();
        backup.reminders.push(bad);
        assert!(svc.import(backup.clone(), ImportMode::Replace, at("09:01")).is_err());
        assert_eq!(svc.list().unwrap().len(), 1);

        backup.reminders.pop();
        backup.app = "otra-app".into();
        assert!(svc.import(backup, ImportMode::Add, at("09:01")).is_err());
        assert_eq!(svc.list().unwrap().len(), 1);
    }

    #[test]
    fn stats_count_answers_and_compute_compliance() {
        let svc = service();
        let r = svc.create(drops(Anchor::Fixed), at("07:00")).unwrap();
        let quiet = svc.create(ReminderSpec { title: "Agua".into(), ..drops(Anchor::Fixed) }, at("07:00")).unwrap();
        svc.act(r.id, Action::Done, at("08:00")).unwrap();
        svc.act(r.id, Action::Snooze { minutes: 5 }, at("10:00")).unwrap();
        svc.act(r.id, Action::Done, at("10:05")).unwrap();
        svc.act(r.id, Action::Done, at("12:00")).unwrap();
        svc.act(r.id, Action::Dismiss, at("14:00")).unwrap();

        let stats = svc.stats(at("09:00")).unwrap();
        let mine = stats.iter().find(|s| s.reminder_id == r.id).unwrap();
        // La de las 08:00 queda fuera del periodo.
        assert_eq!((mine.done, mine.snoozed, mine.dismissed), (2, 1, 1));
        assert_eq!(mine.compliance_pct, Some(67));
        let other = stats.iter().find(|s| s.reminder_id == quiet.id).unwrap();
        assert_eq!(other.compliance_pct, None);
    }

    #[test]
    fn theme_defaults_to_system_and_persists() {
        let svc = service();
        assert_eq!(svc.theme().unwrap(), Theme::System);
        svc.set_theme(Theme::Dark).unwrap();
        assert_eq!(svc.theme().unwrap(), Theme::Dark);
        svc.set_theme(Theme::System).unwrap();
        assert_eq!(svc.theme().unwrap(), Theme::System);
        assert_eq!(serde_json::to_string(&Theme::Light).unwrap(), r#""light""#);
    }

    #[test]
    fn disabled_reminders_never_fire() {
        let svc = service();
        let r = svc.create(drops(Anchor::Fixed), at("09:00")).unwrap();
        let r = svc.set_enabled(r.id, false, at("09:01")).unwrap();
        assert_eq!(r.next_fire_at, None);
        assert!(svc.tick(at("12:00")).unwrap().is_empty());

        let r = svc.set_enabled(r.id, true, at("12:30")).unwrap();
        assert_eq!(r.next_fire_at, Some(at("14:00")));
    }

    #[test]
    fn state_survives_reopening_the_database() {
        let path = std::env::temp_dir().join(format!("nudge-test-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        {
            let svc = Service::new(Store::open(&path).unwrap(), ec());
            svc.create(drops(Anchor::Fixed), at("09:00")).unwrap();
            svc.tick(at("10:00")).unwrap();
        }
        let svc = Service::new(Store::open(&path).unwrap(), ec());
        assert_eq!(svc.pending(at("10:01")).unwrap().len(), 1);
        drop(svc);
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
        }
    }
}
