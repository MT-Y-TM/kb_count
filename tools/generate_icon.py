"""Regenerate the ICO from the project's simple, font-independent SVG mark.

Requires Python + Pillow only for asset maintenance, never at build/runtime.
Run: uv run --with Pillow python tools/generate_icon.py
"""
from pathlib import Path
import xml.etree.ElementTree as ET

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parents[1]
SIZES = (16, 20, 24, 28, 32, 40, 48, 56, 64, 96, 128, 256)
SVG = ET.parse(ROOT / "assets/kb_count.svg").getroot()


def render(size: int) -> Image.Image:
    scale = size * 8 / 64
    image = Image.new("RGBA", (size * 8, size * 8))
    draw = ImageDraw.Draw(image)
    for element in SVG:
        tag = element.tag.rsplit("}", 1)[-1]
        if tag == "rect":
            x, y, width, height, radius = (
                float(element.attrib[key]) * scale
                for key in ("x", "y", "width", "height", "rx")
            )
            draw.rounded_rectangle(
                (x, y, x + width - 1, y + height - 1),
                radius=radius,
                fill=element.attrib["fill"],
            )
        elif tag == "polygon":
            points = [
                tuple(float(value) * scale for value in point.split(","))
                for point in element.attrib["points"].split()
            ]
            draw.polygon(points, fill=element.attrib["fill"])
        else:
            raise ValueError(f"Unsupported SVG element: {tag}")
    return image.resize((size, size), Image.Resampling.LANCZOS)


if __name__ == "__main__":
    images = [render(size) for size in SIZES]
    images[-1].save(
        ROOT / "assets/kb_count.ico",
        sizes=[(size, size) for size in SIZES],
        append_images=images[:-1],
    )
    icon = Image.open(ROOT / "assets/kb_count.ico")
    assert icon.ico.sizes() == {(size, size) for size in SIZES}
    print("Generated assets/kb_count.ico:", ", ".join(map(str, SIZES)))
