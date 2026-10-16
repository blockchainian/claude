"""Builds light.tmTheme and dark.tmTheme, the syntax themes agents embeds, from GitHub's VS Code
themes in github/ (primer/github-vscode-theme 6.3.5, built with @primer/primitives 7.10.0).

Both themes then take the color and style Nvim draws for each scope from nvim.json, measured byte by
byte with the user's Nvim colorscheme over sample files in Rust, JavaScript, Python, shell, JSON and
Markdown; each language-suffixed scope also gives a generic scope without the suffix. Scopes Nvim
was not measured on keep GitHub's colors, which the dark theme maps into DARK_LIGHTNESS (OKLab
lightness, hue kept).

Run: python3 build.py
"""
import json
import plistlib
from pathlib import Path

HERE = Path(__file__).parent
DARK_LIGHTNESS = (0.52, 0.62)
# Nvim's Normal text color on each background, as a palette index.
TEXT = {"light": 30, "dark": 23}


def palette(index):
    return "#%02x000000" % index


def tm_theme(vscode, name):
    colors = vscode["colors"]
    settings = [{"settings": {"background": colors["editor.background"], "foreground": colors["editor.foreground"]}}]
    for rule in vscode["tokenColors"]:
        scope = rule.get("scope")
        if not scope:
            continue
        style = {k: v for k, v in rule["settings"].items() if k in ("foreground", "fontStyle")}
        settings.append({"name": rule.get("name", ""), "scope": scope if isinstance(scope, str) else ", ".join(scope), "settings": style})
    return {"name": name, "settings": settings}


def linear(c):
    return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4


def gamma(c):
    return 12.92 * c if c <= 0.0031308 else 1.055 * c ** (1 / 2.4) - 0.055


def oklab(hex_color):
    r, g, b = (linear(int(hex_color[i:i + 2], 16) / 255) for i in (1, 3, 5))
    l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b) ** (1 / 3)
    m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b) ** (1 / 3)
    s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b) ** (1 / 3)
    return (
        0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s,
        1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s,
        0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s,
    )


def srgb(lightness, a, b):
    l = (lightness + 0.3963377774 * a + 0.2158037573 * b) ** 3
    m = (lightness - 0.1055613458 * a - 0.0638541728 * b) ** 3
    s = (lightness - 0.0894841775 * a - 1.291485548 * b) ** 3
    return (
        4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
        -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
        -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s,
    )


def foregrounds(theme):
    return [rule["settings"] for rule in theme["settings"] if len(rule["settings"].get("foreground", "")) in (7, 9)]


def use_text(theme, text):
    """Replaces the base text color with `text` in every rule that uses it."""
    base = theme["settings"][0]["settings"]["foreground"][:7].lower()
    for style in foregrounds(theme):
        if style["foreground"][:7].lower() == base:
            style["foreground"] = text


def dim(theme, low, high):
    """Maps RGB foreground lightness into [low, high]. Colors already darker than `low`, such as a
    near-background ignored-file color, stay."""
    rgb_styles = [s for s in foregrounds(theme) if len(s["foreground"]) == 7 or s["foreground"][7:].lower() == "ff"]
    lights = [oklab(s["foreground"][:7])[0] for s in rgb_styles]
    lights = [value for value in lights if value > low]
    source_low, source_high = min(lights), max(lights)
    for style in rgb_styles:
        color = style["foreground"]
        lightness, a, b = oklab(color[:7])
        if lightness <= low:
            continue
        target = low + (lightness - source_low) / (source_high - source_low) * (high - low)
        chroma = target / lightness
        while True:
            rgb = srgb(target, a * chroma, b * chroma)
            if all(-1e-6 <= c <= 1 + 1e-6 for c in rgb) or chroma < 1e-3:
                break
            chroma *= 0.97
        style["foreground"] = "#" + "".join("%02x" % round(max(0.0, min(1.0, gamma(max(0.0, c)))) * 255) for c in rgb) + color[7:]


# Type definitions take the function definition style, as Nvim's LSP declarations do.
TYPE_DEFINITIONS = [f"entity.name.{kind}" for kind in ("struct", "enum", "union", "trait", "type", "class", "interface")]


def match_nvim(theme, appearance):
    """Appends one rule per measured scope; `source` raises them above GitHub's own rules."""
    measured = json.loads((HERE / "nvim.json").read_text())
    definition = measured["entity.name.function"]
    for scope in TYPE_DEFINITIONS:
        measured.setdefault(scope, definition)
    for scope, styles in measured.items():
        innermost = scope.split()[-1]
        if any(innermost == name or innermost.startswith(name + ".") for name in TYPE_DEFINITIONS):
            styles = definition
        index, font_style = styles[appearance]
        theme["settings"].append(
            {"name": f"Nvim {scope}", "scope": f"source {scope}, text {scope}", "settings": {"foreground": palette(index), "fontStyle": font_style}}
        )


for appearance in ("light", "dark"):
    theme = tm_theme(json.loads((HERE / "github" / f"{appearance}-default.json").read_text()), appearance)
    use_text(theme, palette(TEXT[appearance]))
    if appearance == "dark":
        dim(theme, *DARK_LIGHTNESS)
    match_nvim(theme, appearance)
    with open(HERE / f"{appearance}.tmTheme", "wb") as file:
        plistlib.dump(theme, file)
