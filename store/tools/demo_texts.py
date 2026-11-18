#!/usr/bin/env python3
"""Turns the demo mode's sample texts of the iOS app
(ios/Ferrowl/Data/DemoTexts.swift) into the Android ones
(android/app/src/main/kotlin/com/ferrowl/app/demo/DemoTexts.kt), so they
are written once for both apps.

    python3 store/tools/demo_texts.py
"""
import re
from pathlib import Path

root = Path(__file__).resolve().parents[2]
source = root / "ios/Ferrowl/Data/DemoTexts.swift"
target = root / "android/app/src/main/kotlin/com/ferrowl/app/demo/DemoTexts.kt"

languages: dict[str, dict[str, str]] = {}
current = None
lines = source.read_text(encoding="utf-8").splitlines()
i = 0
while i < len(lines):
    line = lines[i]
    if m := re.match(r'\s*private static let (\w+): \[String: String\] = \[', line):
        current = languages.setdefault(m.group(1), {})
    elif current is not None and (m := re.match(r'\s*"(\w+)": """\s*$', line)):
        # A multi-line string: the closing """ sets the indentation to remove.
        body = []
        i += 1
        while not lines[i].strip().startswith('"""'):
            body.append(lines[i])
            i += 1
        indent = len(lines[i]) - len(lines[i].lstrip())
        current[m.group(1)] = "\n".join(b[indent:] for b in body)
    elif current is not None and (m := re.match(r'\s*"(\w+)": "(.*)",\s*$', line)):
        current[m.group(1)] = m.group(2)
    elif line.strip() == "]":
        current = None
    i += 1


def kotlin(s: str) -> str:
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"').replace("$", "\\$").replace("\n", "\\n") + '"'


out = [
    "package com.ferrowl.app.demo",
    "",
    "// Generated from ios/Ferrowl/Data/DemoTexts.swift by store/tools/demo_texts.py:",
    "// edit that file and run the script again.",
    "",
    "/** The sample mail of the demo mode (see [Demo]), in every app language. */",
    "internal object DemoTexts {",
    "    fun of(language: String): Map<String, String>? = when (language) {",
]
for code in languages:
    out.append(f'        "{code}" -> {code}()')
out += ["        else -> null", "    }", ""]
for code, texts in languages.items():
    # One function per language: a single initializer would grow too large.
    out.append(f"    private fun {code}() = mapOf(")
    for key, value in texts.items():
        out.append(f'        "{key}" to {kotlin(value)},')
    out += ["    )", ""]
out[-1] = "}"
target.parent.mkdir(parents=True, exist_ok=True)
target.write_text("\n".join(out) + "\n", encoding="utf-8")
counts = ", ".join(f"{code} {len(t)}" for code, t in languages.items())
print(f"{target.relative_to(root)}: {counts}")
