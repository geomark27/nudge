//! Cálculo de la próxima alerta. Todo se guarda en UTC; las ventanas y los días
//! se evalúan en la zona horaria local que se recibe como parámetro.

use chrono::{DateTime, Datelike, Duration, LocalResult, NaiveDate, NaiveDateTime, TimeZone, Utc};

use crate::model::{Anchor, ReminderSpec, Schedule, TimeOfDay};

/// Una semana y un día de margen: suficiente para encontrar el siguiente día activo.
const SEARCH_DAYS: u64 = 8;

/// Próximo momento en que debe sonar `spec`, tomando `now` como referencia.
///
/// Se usa al crear o editar un recordatorio y después de "Hecho" o "Descartar".
/// Las alertas perdidas no se recuperan una por una: siempre se busca hacia adelante.
pub fn next_fire<Tz: TimeZone>(spec: &ReminderSpec, now: DateTime<Utc>, tz: &Tz) -> Option<DateTime<Utc>> {
    match &spec.schedule {
        Schedule::Interval { every_min, anchor: Anchor::FromDone } => {
            fit_into_window(spec, now + Duration::minutes(i64::from(*every_min)), tz)
        }
        _ => next_slot_after(spec, now, tz),
    }
}

/// Horas del día en que suena un recordatorio de horario fijo.
fn daily_slots(spec: &ReminderSpec) -> Vec<TimeOfDay> {
    match &spec.schedule {
        Schedule::Times { times } => {
            let mut times = times.clone();
            times.sort();
            times.dedup();
            times
        }
        Schedule::Interval { every_min, .. } => {
            let (start, end) = (spec.window.start.minutes(), spec.window.end.minutes());
            (start..=end).step_by((*every_min).max(1) as usize).filter_map(TimeOfDay::from_minutes).collect()
        }
    }
}

fn active_days(spec: &ReminderSpec, from: NaiveDate) -> impl Iterator<Item = NaiveDate> + '_ {
    from.iter_days().take(SEARCH_DAYS as usize).filter(|d| spec.days.contains(d.weekday()))
}

/// Primer horario estrictamente posterior a `after`.
fn next_slot_after<Tz: TimeZone>(spec: &ReminderSpec, after: DateTime<Utc>, tz: &Tz) -> Option<DateTime<Utc>> {
    let slots = daily_slots(spec);
    let today = after.with_timezone(tz).date_naive();
    active_days(spec, today)
        .flat_map(|day| slots.iter().map(move |slot| day.and_time(slot.to_naive())))
        .map(|naive| local_to_utc(tz, naive))
        .find(|t| *t > after)
}

/// Primer momento `>= candidate` que cae dentro de la ventana en un día activo.
fn fit_into_window<Tz: TimeZone>(spec: &ReminderSpec, candidate: DateTime<Utc>, tz: &Tz) -> Option<DateTime<Utc>> {
    let day = candidate.with_timezone(tz).date_naive();
    active_days(spec, day).find_map(|day| {
        let start = local_to_utc(tz, day.and_time(spec.window.start.to_naive()));
        let end = local_to_utc(tz, day.and_time(spec.window.end.to_naive()));
        (candidate <= end).then(|| candidate.max(start))
    })
}

/// Convierte una hora local a UTC. Si cae en el hueco de un cambio de horario
/// (p. ej. 02:30 cuando el reloj salta de 02:00 a 03:00) avanza a la primera hora válida;
/// si es ambigua (la hora se repite) toma la primera.
fn local_to_utc<Tz: TimeZone>(tz: &Tz, naive: NaiveDateTime) -> DateTime<Utc> {
    let mut probe = naive;
    for _ in 0..=8 {
        match tz.from_local_datetime(&probe) {
            LocalResult::Single(t) | LocalResult::Ambiguous(t, _) => return t.with_timezone(&Utc),
            LocalResult::None => probe += Duration::minutes(15),
        }
    }
    Utc.from_utc_datetime(&naive)
}

#[cfg(test)]
mod tests {
    use chrono::FixedOffset;

    use super::*;
    use crate::model::{Days, Window};

    /// Ecuador continental: UTC-5, sin horario de verano.
    fn ec() -> FixedOffset {
        FixedOffset::west_opt(5 * 3600).unwrap()
    }

    fn t(s: &str) -> TimeOfDay {
        s.parse().unwrap()
    }

    /// Fecha y hora local de Ecuador → UTC.
    fn at(date: &str, time: &str) -> DateTime<Utc> {
        let naive = NaiveDateTime::parse_from_str(&format!("{date} {time}"), "%Y-%m-%d %H:%M").unwrap();
        ec().from_local_datetime(&naive).unwrap().with_timezone(&Utc)
    }

    fn spec(schedule: Schedule) -> ReminderSpec {
        ReminderSpec {
            title: "Gotas".into(),
            message: String::new(),
            schedule,
            window: Window { start: t("08:00"), end: t("20:00") },
            days: Days::ALL,
            sound: true,
            nag_min: 5,
            enabled: true,
        }
    }

    fn every(min: u32, anchor: Anchor) -> Schedule {
        Schedule::Interval { every_min: min, anchor }
    }

    // 2026-10-20 es martes.

    #[test]
    fn fixed_interval_aligns_to_window_start() {
        let s = spec(every(120, Anchor::Fixed));
        assert_eq!(next_fire(&s, at("2026-10-20", "09:30"), &ec()), Some(at("2026-10-20", "10:00")));
    }

    #[test]
    fn fixed_interval_includes_window_end() {
        let s = spec(every(120, Anchor::Fixed));
        assert_eq!(next_fire(&s, at("2026-10-20", "19:00"), &ec()), Some(at("2026-10-20", "20:00")));
    }

    #[test]
    fn fixed_interval_jumps_to_next_day_after_window() {
        let s = spec(every(120, Anchor::Fixed));
        assert_eq!(next_fire(&s, at("2026-10-20", "20:00"), &ec()), Some(at("2026-10-21", "08:00")));
    }

    #[test]
    fn weekdays_only_skip_the_weekend() {
        let mut s = spec(every(60, Anchor::Fixed));
        s.days = Days::WEEKDAYS;
        // Viernes 23 a las 21:00 → lunes 26 a las 08:00.
        assert_eq!(next_fire(&s, at("2026-10-23", "21:00"), &ec()), Some(at("2026-10-26", "08:00")));
    }

    #[test]
    fn times_are_strictly_after_now_and_ignore_window() {
        let s = spec(Schedule::Times { times: vec![t("22:00"), t("07:00"), t("14:00")] });
        assert_eq!(next_fire(&s, at("2026-10-20", "14:00"), &ec()), Some(at("2026-10-20", "22:00")));
        assert_eq!(next_fire(&s, at("2026-10-20", "22:30"), &ec()), Some(at("2026-10-21", "07:00")));
    }

    #[test]
    fn from_done_counts_from_completion() {
        let s = spec(every(180, Anchor::FromDone));
        assert_eq!(next_fire(&s, at("2026-10-20", "10:15"), &ec()), Some(at("2026-10-20", "13:15")));
    }

    #[test]
    fn from_done_outside_window_moves_to_next_opening() {
        let s = spec(every(180, Anchor::FromDone));
        assert_eq!(next_fire(&s, at("2026-10-20", "19:00"), &ec()), Some(at("2026-10-21", "08:00")));
        // Antes de abrir la ventana espera a que abra.
        assert_eq!(next_fire(&s, at("2026-10-20", "03:00"), &ec()), Some(at("2026-10-20", "08:00")));
    }

    #[test]
    fn no_active_days_means_never() {
        let mut s = spec(every(60, Anchor::Fixed));
        s.days = Days(0);
        assert_eq!(next_fire(&s, at("2026-10-20", "09:00"), &ec()), None);
    }

    #[test]
    fn dst_gap_moves_forward_to_first_valid_time() {
        use chrono_tz::America::New_York;
        // 2026-03-08: en Nueva York el reloj salta de 02:00 a 03:00.
        let s = spec(Schedule::Times { times: vec![t("02:30")] });
        let before = New_York.with_ymd_and_hms(2026, 3, 8, 1, 0, 0).unwrap().with_timezone(&Utc);
        let expected = New_York.with_ymd_and_hms(2026, 3, 8, 3, 0, 0).unwrap().with_timezone(&Utc);
        assert_eq!(next_fire(&s, before, &New_York), Some(expected));
    }
}
