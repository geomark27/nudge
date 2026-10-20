use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, NaiveTime, Utc, Weekday};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::Error;

/// Hora del día con precisión de minutos, serializada como `"HH:MM"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TimeOfDay(u16);

impl TimeOfDay {
    pub fn new(hour: u32, minute: u32) -> Option<Self> {
        (hour < 24 && minute < 60).then(|| Self((hour * 60 + minute) as u16))
    }

    pub fn from_minutes(minutes: u32) -> Option<Self> {
        (minutes < 24 * 60).then_some(Self(minutes as u16))
    }

    pub fn minutes(self) -> u32 {
        self.0 as u32
    }

    pub fn to_naive(self) -> NaiveTime {
        NaiveTime::from_hms_opt(self.minutes() / 60, self.minutes() % 60, 0)
            .expect("TimeOfDay siempre es una hora válida")
    }
}

impl fmt::Display for TimeOfDay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02}:{:02}", self.minutes() / 60, self.minutes() % 60)
    }
}

impl FromStr for TimeOfDay {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let invalid = || Error::Validation(format!("hora inválida: {s:?} (formato HH:MM)"));
        let (h, m) = s.trim().split_once(':').ok_or_else(invalid)?;
        let h = h.parse().map_err(|_| invalid())?;
        let m = m.parse().map_err(|_| invalid())?;
        Self::new(h, m).ok_or_else(invalid)
    }
}

impl Serialize for TimeOfDay {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for TimeOfDay {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

/// Días de la semana activos como máscara de bits (lunes = bit 0 … domingo = bit 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Days(pub u8);

impl Days {
    pub const ALL: Days = Days(0b111_1111);
    pub const WEEKDAYS: Days = Days(0b001_1111);

    pub fn contains(self, day: Weekday) -> bool {
        self.0 & (1 << day.num_days_from_monday()) != 0
    }

    pub fn is_empty(self) -> bool {
        self.0 & Self::ALL.0 == 0
    }
}

/// Desde dónde se cuenta el intervalo de un recordatorio periódico.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Anchor {
    /// Horario fijo alineado al inicio de la ventana: 08:00, 10:00, 12:00…
    Fixed,
    /// Cuenta desde que el usuario marca "Hecho": hecho a las 10:15 → siguiente 13:15.
    FromDone,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Schedule {
    Interval {
        every_min: u32,
        anchor: Anchor,
    },
    /// Horas exactas del día; ignoran la ventana pero respetan los días.
    Times {
        times: Vec<TimeOfDay>,
    },
}

/// Franja horaria en la que un recordatorio puede sonar (ambos extremos incluidos).
/// Si `end < start` cruza la medianoche (22:00–02:00) y pertenece al día en que abre.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Window {
    pub start: TimeOfDay,
    pub end: TimeOfDay,
}

impl Window {
    pub fn crosses_midnight(self) -> bool {
        self.end < self.start
    }

    /// Minutos desde la medianoche del día de apertura hasta el cierre (puede pasar de 1440).
    pub fn end_offset(self) -> u32 {
        self.end.minutes() + if self.crosses_midnight() { 24 * 60 } else { 0 }
    }
}

impl Default for Window {
    fn default() -> Self {
        Self { start: TimeOfDay(0), end: TimeOfDay(23 * 60 + 59) }
    }
}

/// Lo que el usuario configura de un recordatorio.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReminderSpec {
    pub title: String,
    #[serde(default)]
    pub message: String,
    pub schedule: Schedule,
    #[serde(default)]
    pub window: Window,
    pub days: Days,
    #[serde(default = "default_true")]
    pub sound: bool,
    /// Tono integrado (`chime`, `soft`…) o archivo propio (`file:nombre.mp3`).
    #[serde(default = "default_tone")]
    pub tone: String,
    /// Minutos para volver a avisar si no se responde; 0 = no insistir.
    #[serde(default = "default_nag")]
    pub nag_min: u32,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

fn default_nag() -> u32 {
    5
}

fn default_tone() -> String {
    crate::sound::DEFAULT_TONE.to_owned()
}

impl ReminderSpec {
    pub fn validate(&self) -> Result<(), Error> {
        let fail = |msg: &str| Err(Error::Validation(msg.to_owned()));
        if self.title.trim().is_empty() {
            return fail("el título es obligatorio");
        }
        if self.days.is_empty() {
            return fail("selecciona al menos un día");
        }
        if self.window.start == self.window.end {
            return fail("la hora de inicio y la de fin no pueden ser iguales");
        }
        crate::sound::validate_tone(&self.tone)?;
        match &self.schedule {
            Schedule::Interval { every_min, .. } if *every_min == 0 => {
                fail("el intervalo debe ser de al menos 1 minuto")
            }
            Schedule::Times { times } if times.is_empty() => fail("agrega al menos una hora"),
            _ => Ok(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Reminder {
    pub id: i64,
    #[serde(flatten)]
    pub spec: ReminderSpec,
    /// Próximo momento en que debe sonar (o volver a insistir).
    pub next_fire_at: Option<DateTime<Utc>>,
    /// Desde cuándo hay una alerta sin responder.
    pub pending_since: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// Respuesta del usuario a una alerta.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    Done,
    Dismiss,
    Snooze { minutes: u32 },
}

impl Action {
    pub fn as_str(self) -> &'static str {
        match self {
            Action::Done => "done",
            Action::Dismiss => "dismissed",
            Action::Snooze { .. } => "snoozed",
        }
    }
}

/// Tema de la interfaz. `System` sigue el claro/oscuro del sistema operativo.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

impl Theme {
    pub fn as_str(self) -> &'static str {
        match self {
            Theme::System => "system",
            Theme::Light => "light",
            Theme::Dark => "dark",
        }
    }

    /// Valor guardado → tema; lo desconocido vuelve a `System`.
    pub fn parse(s: &str) -> Self {
        match s {
            "light" => Theme::Light,
            "dark" => Theme::Dark,
            _ => Theme::System,
        }
    }
}

/// Alerta que la interfaz debe mostrar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Alert {
    pub reminder_id: i64,
    pub title: String,
    pub message: String,
    pub sound: bool,
    pub tone: String,
    pub due_since: DateTime<Utc>,
    /// Llegó con retraso (equipo suspendido, apagado o pausa): mostrar "pendiente desde…".
    pub late: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Occurrence {
    pub id: i64,
    pub reminder_id: i64,
    pub title: String,
    pub due_at: DateTime<Utc>,
    pub action: String,
    pub acted_at: DateTime<Utc>,
}

/// Archivo de respaldo: la configuración sin estado ni historial.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Backup {
    /// Siempre `"nudge"`; evita importar por error otro JSON.
    pub app: String,
    pub version: u32,
    pub exported_at: DateTime<Utc>,
    #[serde(default)]
    pub theme: Theme,
    pub reminders: Vec<ReminderSpec>,
}

impl Backup {
    pub const APP: &'static str = "nudge";
    pub const VERSION: u32 = 1;
}

/// Qué hacer con los recordatorios actuales al importar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportMode {
    /// Se suman a los que ya hay.
    Add,
    /// Se borran los actuales (y su historial) antes de importar.
    Replace,
}

/// Respuestas a un recordatorio en un periodo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReminderStats {
    pub reminder_id: i64,
    pub title: String,
    pub done: u32,
    pub snoozed: u32,
    pub dismissed: u32,
    /// Hechas sobre respuestas finales (hechas + descartadas); `None` si aún no hay ninguna.
    pub compliance_pct: Option<u8>,
}
