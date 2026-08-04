#!/usr/bin/env python3
"""Checks the committed icon against what the tokens and the generator say.

Split out from `icongen.py` so the architecture check can run it without
importing a module whose import side effects it does not want, and so the
failure names *which* of the three things moved — a check that says only "it
drifted" leaves somebody guessing between a colour change, a code change and a
file somebody edited.
"""

from __future__ import annotations

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import icongen  # noqa: E402  (path is set above)


def main() -> int:
    if not os.path.exists(icongen.MANIFEST):
        print(f"missing {icongen.MANIFEST}")
        return 1

    recorded: dict[str, str] = {}
    with open(icongen.MANIFEST, encoding="utf-8") as handle:
        for line in handle:
            parts = line.split()
            if not parts or parts[0].startswith("#"):
                continue
            if parts[0] == "image":
                recorded[f"image {parts[1]}"] = parts[2]
            else:
                recorded[parts[0]] = parts[1]

    import json

    problems: list[str] = []

    expected = icongen.digest(
        json.dumps(icongen._tokens()["color"], sort_keys=True).encode()
    )
    if recorded.get("tokens") != expected:
        problems.append("the design tokens changed since the icon was drawn")

    with open(icongen.__file__, "rb") as handle:
        if recorded.get("generator") != icongen.digest(handle.read()):
            problems.append("the generator changed since the icon was drawn")

    target = os.path.join(
        icongen.ROOT, "apple", "Resources", "Assets.xcassets", "AppIcon.appiconset"
    )
    present = sorted(name for name in os.listdir(target) if name.endswith(".png"))
    for name in present:
        with open(os.path.join(target, name), "rb") as handle:
            actual = icongen.digest(handle.read())
        if f"image {name}" not in recorded:
            problems.append(f"{name} is not in the manifest")
        elif recorded[f"image {name}"] != actual:
            problems.append(f"{name} does not match the manifest")
    for key in recorded:
        if key.startswith("image ") and key.split(" ", 1)[1] not in present:
            problems.append(f"{key.split(' ', 1)[1]} is in the manifest but missing")

    for problem in problems:
        print(problem)
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
