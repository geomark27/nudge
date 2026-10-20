//! Tonos de alerta: los integrados se sintetizan en la UI; los propios son archivos de audio
//! copiados a la carpeta `sounds/` de la app y referenciados como `file:<nombre>`.

use crate::Error;

pub const DEFAULT_TONE: &str = "chime";
/// Debe coincidir con `TONES` en `ui/common.js`.
pub const BUILTIN_TONES: &[&str] = &["chime", "soft", "double", "drop", "alarm"];
pub const FILE_PREFIX: &str = "file:";
/// Tamaño máximo de un sonido propio.
pub const MAX_SOUND_BYTES: u64 = 10 * 1024 * 1024;

const EXTENSIONS: &[(&str, &str)] =
    &[("mp3", "audio/mpeg"), ("wav", "audio/wav"), ("ogg", "audio/ogg"), ("m4a", "audio/mp4")];
const MAX_STEM_CHARS: usize = 60;

pub fn validate_tone(tone: &str) -> Result<(), Error> {
    let ok = match tone.strip_prefix(FILE_PREFIX) {
        Some(name) => is_safe_file_name(name),
        None => BUILTIN_TONES.contains(&tone),
    };
    if ok { Ok(()) } else { Err(Error::Validation(format!("tono desconocido: {tone:?}"))) }
}

/// Extensiones admitidas, para el filtro del diálogo de archivos.
pub fn extensions() -> impl Iterator<Item = &'static str> {
    EXTENSIONS.iter().map(|(ext, _)| *ext)
}

/// Tipo MIME de un sonido propio a partir de su extensión.
pub fn mime_type(name: &str) -> Option<&'static str> {
    mime_for_extension(&extension(name)?)
}

/// Nombre que se puede usar dentro de la carpeta de sonidos sin salir de ella:
/// solo letras, números, `-`, `_` y `.`, sin empezar por punto y con extensión admitida.
pub fn is_safe_file_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('.')
        && !name.contains("..")
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        && mime_type(name).is_some()
}

/// Convierte el nombre del archivo elegido en uno seguro y libre dentro de la carpeta.
/// `taken` dice si un nombre ya existe; se añade `-2`, `-3`… hasta encontrar uno libre.
pub fn sound_file_name(original: &str, taken: impl Fn(&str) -> bool) -> Result<String, Error> {
    let ext = extension(original).filter(|ext| mime_for_extension(ext).is_some()).ok_or_else(|| {
        Error::Validation(format!("formato no admitido; usa {}", extensions().collect::<Vec<_>>().join(", ")))
    })?;
    let stem = original.rsplit_once('.').map_or(original, |(stem, _)| stem);
    let mut clean = String::new();
    for c in stem.chars().flat_map(char::to_lowercase) {
        match c {
            c if c.is_ascii_alphanumeric() => clean.push(c),
            'á' | 'à' | 'ä' => clean.push('a'),
            'é' | 'è' | 'ë' => clean.push('e'),
            'í' | 'ì' | 'ï' => clean.push('i'),
            'ó' | 'ò' | 'ö' => clean.push('o'),
            'ú' | 'ù' | 'ü' => clean.push('u'),
            'ñ' => clean.push('n'),
            _ if !clean.ends_with('-') => clean.push('-'),
            _ => {}
        }
    }
    let clean: String = clean.trim_matches('-').chars().take(MAX_STEM_CHARS).collect();
    let stem = if clean.is_empty() { "sonido".to_owned() } else { clean };
    let candidate = |n: u32| if n == 1 { format!("{stem}.{ext}") } else { format!("{stem}-{n}.{ext}") };
    (1..1000)
        .map(candidate)
        .find(|name| !taken(name))
        .ok_or_else(|| Error::Validation("hay demasiados sonidos con ese nombre".into()))
}

fn mime_for_extension(ext: &str) -> Option<&'static str> {
    EXTENSIONS.iter().find(|(e, _)| *e == ext).map(|(_, mime)| *mime)
}

fn extension(name: &str) -> Option<String> {
    name.rsplit_once('.').map(|(_, ext)| ext.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_builtin_and_file_tones() {
        assert!(validate_tone("chime").is_ok());
        assert!(validate_tone("file:agua.mp3").is_ok());
        assert!(validate_tone("trompeta").is_err());
        assert!(validate_tone("file:../nudge.db").is_err());
        assert!(validate_tone("file:notas.txt").is_err());
    }

    #[test]
    fn rejects_names_that_escape_the_folder() {
        for bad in ["../x.mp3", "a/b.mp3", r"a\b.mp3", ".oculto.mp3", "", "x.exe"] {
            assert!(!is_safe_file_name(bad), "{bad:?} debería rechazarse");
        }
        assert!(is_safe_file_name("campana-2.ogg"));
    }

    #[test]
    fn sanitizes_and_deduplicates_file_names() {
        let none = |_: &str| false;
        assert_eq!(sound_file_name("Canción Bonita (1).MP3", none).unwrap(), "cancion-bonita-1.mp3");
        assert_eq!(sound_file_name("###.wav", none).unwrap(), "sonido.wav");
        let taken = |n: &str| n == "gota.ogg" || n == "gota-2.ogg";
        assert_eq!(sound_file_name("gota.ogg", taken).unwrap(), "gota-3.ogg");
        assert!(sound_file_name("documento.pdf", none).is_err());
        assert!(sound_file_name("sin-extension", none).is_err());
    }

    #[test]
    fn mime_type_follows_extension() {
        assert_eq!(mime_type("a.MP3"), Some("audio/mpeg"));
        assert_eq!(mime_type("a.flac"), None);
    }
}
