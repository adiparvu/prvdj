#!/usr/bin/env python3
"""Draws the application icon from the design tokens.

# Why the icon is generated rather than drawn

Master Prompt #17 makes `design/tokens/tokens.json` the single source of truth
for colour, and an icon pasted in from a design tool is a second source that
nobody notices has drifted. Generating it means the accent in the icon is the
same value as the accent in the interface, by construction — and a token change
is one command away from a matching icon rather than a task somebody forgets.

# What it draws

A waveform whose tone changes partway across: the record going out on the left,
the record coming in on the right, and the change itself is the handover. That
is the whole product in one shape, and it survives being sixteen pixels wide
because it is three tones and no detail.

# Why the two platforms get different files

macOS bakes the rounded-rectangle shape and its margin into the image; iOS
supplies its own mask and requires a full-bleed opaque square. An icon drawn once
and used for both is wrong on one of them, so this draws both.

No third-party imaging library: PNG is a container around zlib, and writing one
directly costs forty lines and removes a dependency from the build.
"""

from __future__ import annotations

import hashlib
import json
import math
import os
import struct
import sys
import zlib

def samples_for(size: int) -> int:
    """How far to supersample an icon of this size.

    Antialiasing only matters where an edge is not axis-aligned, and the only
    such edge here is the rounded corner. At sixteen pixels that corner is the
    whole shape and needs every sample it can get; at a thousand it is a smooth
    arc either way.

    Scaling this is not a nicety. A flat factor of four means the largest icon
    is sixteen million samples of interpreted Python — about a minute — and a
    generator nobody will wait for is a generator that stops being run.
    """
    if size <= 128:
        return 4
    if size <= 512:
        return 3
    return 2

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
TOKENS = os.path.join(ROOT, "design", "tokens", "tokens.json")


def token(name: str, appearance: str = "dark") -> str:
    """Reads one colour from the design tokens.

    Token names contain dots — `surface.canvas` is one key, not two — so the
    name is passed whole rather than split. Getting that wrong reads a token
    that does not exist, which is a loud failure and the right kind.
    """
    return _tokens()["color"]["tokens"][name][appearance]


_CACHE: dict | None = None


def _tokens() -> dict:
    global _CACHE
    if _CACHE is None:
        with open(TOKENS, encoding="utf-8") as handle:
            _CACHE = json.load(handle)
    return _CACHE


def rgb(value: str) -> tuple[int, int, int]:
    value = value.lstrip("#")
    return (int(value[0:2], 16), int(value[2:4], 16), int(value[4:6], 16))


def mix(a: tuple[int, int, int], b: tuple[int, int, int], t: float) -> tuple[int, int, int]:
    return tuple(round(a[i] + (b[i] - a[i]) * t) for i in range(3))


def png(width: int, height: int, pixels: list[tuple[int, int, int]]) -> bytes:
    """Encodes RGB pixels as a PNG."""
    raw = bytearray()
    for y in range(height):
        raw.append(0)  # filter type 0: none
        for x in range(width):
            raw.extend(pixels[y * width + x])

    def chunk(kind: bytes, data: bytes) -> bytes:
        return (
            struct.pack(">I", len(data))
            + kind
            + data
            + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)
        )

    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(bytes(raw), 9))
        + chunk(b"IEND", b"")
    )


# The bars, as fractions of the icon's height. Deliberately asymmetric: a
# symmetrical waveform reads as a graphic, and an uneven one reads as music.
BARS = [0.30, 0.52, 0.92, 0.56, 0.34]

# Which bar the tone changes on. Everything before it is the record going out;
# everything from it on is the record coming in, and the change of tone is the
# handover. One bar carries both roles because that is what a transition is.
HANDOVER = 2


def draw(size: int, rounded: bool) -> bytes:
    """Renders one icon.

    `rounded` bakes in the macOS shape and margin. iOS masks its own, and an
    icon that arrived already rounded would be rounded twice.
    """
    canvas = rgb(token("surface.canvas"))
    raised = rgb(token("surface.raised"))
    accent = rgb(token("accent.primary"))
    content = rgb(token("content.primary"))

    factor = samples_for(size)
    big = size * factor
    # macOS leaves roughly a tenth of the canvas clear on each side; iOS fills
    # it entirely.
    margin = big * 0.10 if rounded else 0.0
    inner = big - 2 * margin
    radius = inner * 0.225 if rounded else 0.0

    field: list[tuple[int, int, int]] = []
    bar_count = len(BARS)
    # Bars occupy the middle two thirds of the plate, leaving the shape room to
    # breathe at every size Apple asks for.
    span = inner * 0.62
    bar_width = span / (bar_count * 2 - 1)
    origin = margin + (inner - span) / 2.0

    for y in range(big):
        for x in range(big):
            colour = canvas

            within = (
                margin <= x < margin + inner
                and margin <= y < margin + inner
                and _inside_rounded(x, y, margin, inner, radius)
            )
            if within:
                # A vertical wash so the plate has depth at large sizes and
                # still reads as one tone at sixteen pixels.
                depth = (y - margin) / inner
                colour = mix(raised, canvas, depth * 0.55)

                index = _bar_at(x, origin, bar_width, bar_count)
                if index is not None:
                    height = BARS[index] * inner * 0.62
                    centre = margin + inner / 2.0
                    if centre - height / 2 <= y <= centre + height / 2:
                        # The record coming in carries the accent; the one going
                        # out is plain. Two tones, so the direction of the mix
                        # is legible without a legend — and at sixteen pixels
                        # the tones are all that survives, which is the test.
                        colour = accent if index >= HANDOVER else content

            field.append(colour)

    # Box-filter down from the supersampled field.
    out: list[tuple[int, int, int]] = []
    for y in range(size):
        for x in range(size):
            total = [0, 0, 0]
            for dy in range(factor):
                for dx in range(factor):
                    pixel = field[(y * factor + dy) * big + (x * factor + dx)]
                    for channel in range(3):
                        total[channel] += pixel[channel]
            out.append(tuple(value // (factor * factor) for value in total))

    return png(size, size, out)


def _inside_rounded(x: float, y: float, margin: float, inner: float, radius: float) -> bool:
    if radius <= 0:
        return True
    left, top = margin + radius, margin + radius
    right, bottom = margin + inner - radius, margin + inner - radius
    cx = min(max(x, left), right)
    cy = min(max(y, top), bottom)
    return math.hypot(x - cx, y - cy) <= radius


def _bar_at(x: float, origin: float, bar_width: float, bar_count: int) -> int | None:
    """Which bar a column belongs to, or `None` for the gaps between them."""
    offset = x - origin
    if offset < 0:
        return None
    slot = int(offset // bar_width)
    if slot >= bar_count * 2 - 1 or slot % 2 == 1:
        return None
    return slot // 2


# What each platform's asset catalogue asks for.
MACOS_SIZES = [16, 32, 64, 128, 256, 512, 1024]
IOS_SIZES = [1024]


MANIFEST = os.path.join(ROOT, "design", "tokens", "icon-manifest.txt")


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def write_manifest(target: str, names: list[str]) -> None:
    """Records what the icons were made from, and what they came out as.

    The architecture check compares these hashes rather than re-rendering.
    Re-rendering is twenty seconds of interpreted Python; hashing is
    milliseconds, and it catches strictly more: a changed token, a changed
    generator, *and* an image somebody edited by hand.
    """
    lines = [
        "# Written by tools/icongen.py. Checked by tools/check-architecture.sh.",
        "#",
        "# The icon is generated from the design tokens, so three things have to",
        "# agree: the colours it was drawn from, the code that drew it, and the",
        "# images themselves. Any one of them moving without the others is drift.",
        "",
        f"tokens {digest(json.dumps(_tokens()['color'], sort_keys=True).encode())}",
        f"generator {digest(open(__file__, 'rb').read())}",
    ]
    for name in names:
        with open(os.path.join(target, name), "rb") as handle:
            lines.append(f"image {name} {digest(handle.read())}")
    with open(MANIFEST, "w", encoding="utf-8") as handle:
        handle.write("\n".join(lines) + "\n")


def main() -> int:
    target = os.path.join(ROOT, "apple", "Resources", "Assets.xcassets", "AppIcon.appiconset")
    os.makedirs(target, exist_ok=True)

    for size in sorted(set(MACOS_SIZES)):
        with open(os.path.join(target, f"mac-{size}.png"), "wb") as handle:
            handle.write(draw(size, rounded=True))
    for size in IOS_SIZES:
        with open(os.path.join(target, f"ios-{size}.png"), "wb") as handle:
            handle.write(draw(size, rounded=False))

    names = sorted(name for name in os.listdir(target) if name.endswith(".png"))
    write_manifest(target, names)

    print(f"wrote {len(names)} icons to {target}")
    print(f"wrote {MANIFEST}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
