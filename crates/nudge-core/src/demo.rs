//! Datos de ejemplo para desarrollo: recordatorios variados y semanas de historial con distintos
//! niveles de cumplimiento. La app solo los carga en builds de depuración con `NUDGE_SEED=1`
//! (`make dev-seed`), sobre la base de datos de desarrollo; un release no los enlaza.

use chrono::{DateTime, Duration, TimeZone, Utc};

use crate::Error;
use crate::model::{Action, Anchor, Days, ReminderSpec, Schedule, Window};
use crate::schedule::next_fire;
use crate::service::Service;

/// Días hacia atrás para los que se genera historial.
const HISTORY_DAYS: i64 = 30;

impl<Tz: TimeZone> Service<Tz> {
    /// Reemplaza todos los recordatorios por los de ejemplo, con su historial, y quita la pausa.
    /// Devuelve cuántas respuestas generó. Es determinista: la misma `now` da los mismos datos.
    pub fn load_demo(&self, now: DateTime<Utc>) -> Result<usize, Error> {
        let mut rng = Lcg(42);
        let answers = self.store.in_transaction(|store| {
            store.delete_all()?;
            let mut answers = 0;
            for (spec, diligence) in demo_reminders() {
                let id = store.insert(&spec, self.schedule_next(&spec, now), now)?;
                let mut t = now - Duration::days(HISTORY_DAYS);
                while let Some(due) = next_fire(&spec, t, &self.tz).filter(|due| *due < now) {
                    // Cada recordatorio tiene su "constancia": así el cumplimiento sale variado.
                    let roll = rng.next();
                    let missed = 1.0 - diligence;
                    let mut acted = due + rng.minutes(4);
                    let mut actions = Vec::new();
                    if roll < diligence {
                        actions.push((Action::Done, acted));
                    } else if roll < diligence + missed * 0.5 {
                        actions.push((Action::Snooze { minutes: 5 }, acted));
                        acted += Duration::minutes(5) + rng.minutes(2);
                        let after = if rng.next() < diligence { Action::Done } else { Action::Dismiss };
                        actions.push((after, acted));
                    } else if roll < diligence + missed * 0.8 {
                        actions.push((Action::Dismiss, acted));
                    } // si no, quedó sin respuesta
                    if acted >= now {
                        break;
                    }
                    for (action, at) in actions {
                        store.add_occurrence(id, due, action.as_str(), at)?;
                        answers += 1;
                    }
                    t = acted.max(due);
                }
            }
            Ok(answers)
        })?;
        self.pause_until(None)?;
        Ok(answers)
    }
}

/// Recordatorios de ejemplo con su constancia (probabilidad de marcarlo hecho a la primera).
/// Son más de una página y cubren todos los tipos: intervalo fijo, desde "hecho", horas exactas,
/// franja que cruza la medianoche, días laborables y uno desactivado.
fn demo_reminders() -> Vec<(ReminderSpec, f64)> {
    let window = |start: &str, end: &str| Window { start: start.parse().unwrap(), end: end.parse().unwrap() };
    let times = |list: &[&str]| Schedule::Times { times: list.iter().map(|t| t.parse().unwrap()).collect() };
    let base = ReminderSpec {
        title: String::new(),
        message: String::new(),
        schedule: Schedule::Interval { every_min: 60, anchor: Anchor::Fixed },
        window: window("08:00", "20:00"),
        days: Days::ALL,
        sound: true,
        tone: "chime".into(),
        nag_min: 5,
        enabled: true,
    };
    vec![
        (
            ReminderSpec {
                title: "Gotas para los ojos".into(),
                message: "Una gota en cada ojo".into(),
                schedule: Schedule::Interval { every_min: 120, anchor: Anchor::Fixed },
                tone: "drop".into(),
                ..base.clone()
            },
            0.85,
        ),
        (
            ReminderSpec {
                title: "Tomar agua".into(),
                message: "Un vaso completo".into(),
                schedule: Schedule::Interval { every_min: 60, anchor: Anchor::FromDone },
                window: window("09:00", "18:00"),
                days: Days::WEEKDAYS,
                ..base.clone()
            },
            0.6,
        ),
        (
            ReminderSpec {
                title: "Pausa activa".into(),
                message: "Levántate y camina 2 minutos".into(),
                schedule: Schedule::Interval { every_min: 50, anchor: Anchor::Fixed },
                window: window("08:00", "17:00"),
                days: Days::WEEKDAYS,
                tone: "soft".into(),
                nag_min: 0,
                ..base.clone()
            },
            0.3,
        ),
        (
            ReminderSpec {
                title: "Medicamento".into(),
                message: "Según la pauta médica".into(),
                schedule: times(&["08:00", "20:00"]),
                tone: "alarm".into(),
                nag_min: 10,
                ..base.clone()
            },
            0.95,
        ),
        (
            ReminderSpec {
                title: "Estirar la espalda".into(),
                schedule: Schedule::Interval { every_min: 90, anchor: Anchor::Fixed },
                window: window("10:00", "16:00"),
                days: Days::WEEKDAYS,
                tone: "double".into(),
                ..base.clone()
            },
            0.5,
        ),
        (
            ReminderSpec {
                title: "Leer antes de dormir".into(),
                message: "Cierra las pantallas".into(),
                schedule: Schedule::Interval { every_min: 60, anchor: Anchor::Fixed },
                window: window("21:00", "01:00"),
                nag_min: 15,
                ..base.clone()
            },
            0.7,
        ),
        (
            ReminderSpec {
                title: "Vitamina D".into(),
                schedule: times(&["13:00"]),
                days: Days::WEEKDAYS,
                enabled: false,
                ..base
            },
            0.75,
        ),
    ]
}

/// Generador pseudoaleatorio mínimo (LCG): basta para variar los datos sin otra dependencia.
struct Lcg(u64);

impl Lcg {
    /// Número en `[0, 1)`.
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Entre 0 y `max` minutos.
    fn minutes(&mut self, max: i64) -> Duration {
        Duration::minutes((self.next() * (max + 1) as f64) as i64)
    }
}

#[cfg(test)]
mod tests {
    use chrono::{FixedOffset, NaiveDateTime};

    use super::*;
    use crate::store::Store;

    fn ec() -> FixedOffset {
        FixedOffset::west_opt(5 * 3600).unwrap()
    }

    fn now() -> DateTime<Utc> {
        let naive = NaiveDateTime::parse_from_str("2026-10-20 15:30", "%Y-%m-%d %H:%M").unwrap();
        ec().from_local_datetime(&naive).unwrap().with_timezone(&Utc)
    }

    fn service() -> Service<FixedOffset> {
        Service::new(Store::open_in_memory().unwrap(), ec())
    }

    #[test]
    fn demo_specs_are_valid() {
        for (spec, _) in demo_reminders() {
            spec.validate().unwrap_or_else(|e| panic!("{}: {e}", spec.title));
        }
    }

    #[test]
    fn loads_reminders_and_past_history() {
        let svc = service();
        let answers = svc.load_demo(now()).unwrap();

        let reminders = svc.list().unwrap();
        assert_eq!(reminders.len(), demo_reminders().len());
        assert!(reminders.iter().any(|r| !r.spec.enabled));
        assert!(reminders.iter().all(|r| r.pending_since.is_none()));
        assert!(reminders.iter().filter(|r| r.spec.enabled).all(|r| r.next_fire_at.unwrap() > now()));

        assert!(answers > 500, "historial escaso: {answers}");
        let history = svc.history(u32::MAX).unwrap();
        assert_eq!(history.len(), answers);
        assert!(history.iter().all(|o| o.acted_at < now() && o.acted_at >= o.due_at));
        assert!(history.iter().all(|o| o.acted_at > now() - Duration::days(HISTORY_DAYS)));
        for action in ["done", "snoozed", "dismissed"] {
            assert!(history.iter().any(|o| o.action == action), "falta {action}");
        }
    }

    #[test]
    fn compliance_varies_between_reminders() {
        let svc = service();
        svc.load_demo(now()).unwrap();
        let pcts: Vec<u8> =
            svc.stats(now() - Duration::days(7)).unwrap().iter().filter_map(|s| s.compliance_pct).collect();
        assert!(pcts.iter().any(|p| *p >= 80), "{pcts:?}");
        assert!(pcts.iter().any(|p| *p < 50), "{pcts:?}");
    }

    #[test]
    fn reloading_replaces_everything_and_is_deterministic() {
        let svc = service();
        let first = svc.load_demo(now()).unwrap();
        svc.pause_until(Some(now() + Duration::minutes(30))).unwrap();
        let second = svc.load_demo(now()).unwrap();

        assert_eq!(first, second);
        assert_eq!(svc.list().unwrap().len(), demo_reminders().len());
        assert_eq!(svc.history(u32::MAX).unwrap().len(), second);
        assert_eq!(svc.paused_until(now()).unwrap(), None);
    }
}
