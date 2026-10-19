"""Rasterize our terminal-cell SVG captures using Pillow and a local monospace font.

Only local rect/text elements emitted by buffer_svg are supported; not a general SVG renderer.
Usage: python scripts/render-captures.py docs/screenshots
"""
from pathlib import Path
import sys
import xml.etree.ElementTree as ET
from PIL import Image, ImageDraw, ImageFont

directory = Path(sys.argv[1] if len(sys.argv) > 1 else "docs/screenshots")
font_path = Path("C:/Windows/Fonts/consola.ttf")
if not font_path.exists():
    font_path = Path("/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf")
font = ImageFont.truetype(str(font_path), 14)
bold_path = font_path.with_name("consolab.ttf") if sys.platform == "win32" else font_path.with_name("DejaVuSansMono-Bold.ttf")
bold = ImageFont.truetype(str(bold_path if bold_path.exists() else font_path), 14)
symbol_path = Path("C:/Windows/Fonts/seguisym.ttf")
symbols = ImageFont.truetype(str(symbol_path), 16) if symbol_path.exists() else font
for path in sorted(directory.glob("*.svg")):
    root = ET.parse(path).getroot()
    image = Image.new("RGB", (int(root.attrib["width"]), int(root.attrib["height"])), "#090f1a")
    draw = ImageDraw.Draw(image)
    for element in root.iter():
        tag = element.tag.rsplit("}", 1)[-1]
        a = element.attrib
        if tag == "rect" and a.get("width", "").isdigit():
            x, y = int(a.get("x", 0)), int(a.get("y", 0))
            draw.rectangle((x, y, x + int(a["width"]) - 1, y + int(a["height"]) - 1), fill=a["fill"])
        elif tag == "text":
            text = element.text or ""
            chosen = symbols if any(0x2500 <= ord(c) <= 0x259f for c in text) else bold if a.get("font-weight") == "700" else font
            draw.text((int(a["x"]), int(a["y"])), text, fill=a["fill"], font=chosen, anchor="ls")
    image.save(path.with_suffix(".png"))
    print(path.with_suffix(".png"))
