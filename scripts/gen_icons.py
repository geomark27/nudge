#!/usr/bin/env python3
"""Genera los íconos provisionales de Nudge (PNG + ICO) sin dependencias externas.

Uso: python3 scripts/gen_icons.py
Para el ícono definitivo, crea un PNG de 1024x1024 y ejecuta `cargo tauri icon <png>`,
que además genera el .icns de macOS.
"""

import math
import struct
import zlib
from pathlib import Path

OUT = Path(__file__).resolve().parent.parent / "src-tauri" / "icons"
TOP = (99, 102, 241)  # índigo
BOTTOM = (124, 58, 237)  # violeta
SAMPLES = 4  # supermuestreo por eje para suavizar bordes


def coverage(x: float, y: float) -> tuple[float, float]:
    """Devuelve (fondo, blanco) en coordenadas normalizadas 0..1."""
    r, inset = 0.22, 0.02
    lo, hi = inset, 1 - inset
    cx = min(max(x, lo + r), hi - r)
    cy = min(max(y, lo + r), hi - r)
    bg = 1.0 if math.hypot(x - cx, y - cy) <= r and lo <= x <= hi and lo <= y <= hi else 0.0
    d = math.hypot(x - 0.5, y - 0.5)
    ring = 0.235 <= d <= 0.305
    dot = d <= 0.115
    return bg, 1.0 if bg and (ring or dot) else 0.0


def render(size: int) -> bytes:
    rows = []
    for py in range(size):
        row = bytearray([0])  # filtro PNG "None"
        for px in range(size):
            bg = white = 0.0
            for sy in range(SAMPLES):
                for sx in range(SAMPLES):
                    b, w = coverage((px + (sx + 0.5) / SAMPLES) / size, (py + (sy + 0.5) / SAMPLES) / size)
                    bg += b
                    white += w
            n = SAMPLES * SAMPLES
            bg, white = bg / n, white / n
            t = py / max(size - 1, 1)
            base = [TOP[i] + (BOTTOM[i] - TOP[i]) * t for i in range(3)]
            mix = white / bg if bg else 0
            rgb = [round(c + (255 - c) * mix) for c in base]
            row += bytes(rgb + [round(255 * bg)])
        rows.append(bytes(row))
    return png(size, b"".join(rows))


def png(size: int, raw: bytes) -> bytes:
    def chunk(kind: bytes, data: bytes) -> bytes:
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))

    ihdr = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr) + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b"")


def ico(images: dict[int, bytes]) -> bytes:
    header = struct.pack("<HHH", 0, 1, len(images))
    offset = 6 + 16 * len(images)
    entries, data = b"", b""
    for size, blob in images.items():
        dim = 0 if size >= 256 else size
        entries += struct.pack("<BBBBHHII", dim, dim, 0, 0, 1, 32, len(blob), offset + len(data))
        data += blob
    return header + entries + data


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    cache = {s: render(s) for s in (16, 24, 32, 48, 64, 128, 256, 512)}
    (OUT / "32x32.png").write_bytes(cache[32])
    (OUT / "128x128.png").write_bytes(cache[128])
    (OUT / "128x128@2x.png").write_bytes(cache[256])
    (OUT / "icon.png").write_bytes(cache[512])
    (OUT / "icon.ico").write_bytes(ico({s: cache[s] for s in (16, 24, 32, 48, 64, 256)}))
    print(f"íconos generados en {OUT}")


if __name__ == "__main__":
    main()
