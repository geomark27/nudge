# Bitácora de cambios

Todos los cambios relevantes de Nudge se anotan aquí. El formato sigue
[Keep a Changelog](https://keepachangelog.com/es-ES/1.1.0/) y las versiones siguen
[Versionado Semántico](https://semver.org/lang/es/): `MAYOR.MENOR.PARCHE`.

Mientras trabajas, anota los cambios en **[Unreleased]** bajo *Añadido*, *Cambiado*, *Corregido* o
*Eliminado*. `make release VERSION=x.y.z` convierte esa sección en la de la versión y la usa como
notas del release en GitHub.

## [Unreleased]

### Añadido

- Recordatorios recurrentes con tres tipos de horario: cada cierto tiempo en horario fijo, cada cierto
  tiempo desde que se marca "Hecho" y a horas exactas.
- Franja horaria y días de la semana por recordatorio; la franja puede cruzar la medianoche
  (22:00–02:00) y cuenta para el día en que empieza.
- Ventana de alerta siempre encima con Hecho, +5 min, +10 min y Descartar, con notificación nativa
  de respaldo.
- Insistencia configurable si no se responde. Tras una suspensión o un apagado sale una sola alerta
  "pendiente desde…", no una por cada horario perdido.
- Pausa global de 30 minutos o 1 hora desde la ventana principal o la bandeja del sistema.
- Ícono en la bandeja, instancia única e inicio con el sistema (arranca oculto).
- Tema claro, oscuro o el del sistema, aplicado también a la barra de título.
- Cinco tonos integrados (Campanita, Suave, Doble toque, Gota, Alarma) y sonidos propios en MP3, WAV,
  OGG o M4A, con botón para probarlos.
- Cumplimiento por recordatorio en los últimos 7 o 30 días, e historial reciente de respuestas.
- Exportar e importar la configuración en JSON, añadiendo o reemplazando los recordatorios actuales.
- Instaladores para Windows (MSI y NSIS), Linux (DEB, RPM y AppImage) y macOS (DMG universal),
  compilados y publicados por GitHub Actions.
- Instalación en una línea: `install.ps1` para Windows e `install.sh` para Linux y macOS.
- Actualización automática: Nudge busca versiones nuevas al abrir y cada 6 horas, avisa y se
  actualiza con un clic verificando la firma del paquete.

[Unreleased]: https://github.com/geomark27/nudge/commits/main
