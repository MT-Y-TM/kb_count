"""Inspect the shipped PE without launching it or reading application data.

Run: uv run python tools/verify_release.py [path/to/kb_count.exe]
Uses only the Python standard library.
"""
from pathlib import Path
import json
import struct
import sys

ROOT = Path(__file__).resolve().parents[1]
EXPECTED_SIZES = {16, 20, 24, 28, 32, 40, 48, 56, 64, 96, 128, 256}


def verify(path: Path) -> dict:
    data = path.read_bytes()

    def word(offset):
        return struct.unpack_from("<H", data, offset)[0]

    def dword(offset):
        return struct.unpack_from("<I", data, offset)[0]

    pe = dword(0x3C)
    assert data[:2] == b"MZ" and data[pe:pe + 4] == b"PE\0\0", "Not a PE file"
    assert word(pe + 4) == 0x8664, "Expected Windows x64"
    optional = pe + 24
    assert word(optional) == 0x20B, "Expected PE32+"
    assert word(optional + 68) == 2, "Release must use the Windows GUI subsystem"
    section_table = optional + word(pe + 20)
    sections = [
        (dword(pos + 12), max(dword(pos + 8), dword(pos + 16)), dword(pos + 20))
        for pos in range(section_table, section_table + word(pe + 6) * 40, 40)
    ]

    def file_offset(rva):
        for address, size, offset in sections:
            if address <= rva < address + size:
                return offset + rva - address
        raise ValueError(f"Unmapped resource RVA {rva:#x}")

    resource = file_offset(dword(optional + 112 + 2 * 8))

    def entries(directory):
        pos = resource + directory
        count = word(pos + 12) + word(pos + 14)
        return {dword(pos + 16 + i * 8): dword(pos + 20 + i * 8) for i in range(count)}

    def payload(kind, identifier):
        types = entries(0)
        identifiers = entries(types[kind] & 0x7FFFFFFF)
        languages = entries(identifiers[identifier] & 0x7FFFFFFF)
        entry = resource + next(iter(languages.values()))
        offset, size = file_offset(dword(entry)), dword(entry + 4)
        return data[offset:offset + size]

    group = payload(14, 1)  # RT_GROUP_ICON, resource ID 1
    assert struct.unpack_from("<HH", group) == (0, 1), "Invalid icon group"
    count = struct.unpack_from("<H", group, 4)[0]
    sizes = set()
    for index in range(count):
        pos = 6 + index * 14
        width, height = group[pos] or 256, group[pos + 1] or 256
        assert width == height, "Expected square brand icons"
        expected_bytes, identifier = struct.unpack_from("<IH", group, pos + 8)
        image = payload(3, identifier)  # RT_ICON
        assert len(image) == expected_bytes and len(image) > 0, "Missing icon payload"
        sizes.add(width)
    assert sizes == EXPECTED_SIZES and count == len(EXPECTED_SIZES), "Incomplete icon sizes"
    assert payload(16, 1), "Missing version resource"
    return {"path": str(path), "architecture": "x64", "subsystem": "GUI",
            "icon_sizes": sorted(sizes), "version_resource": True, "bytes": len(data)}


if __name__ == "__main__":
    artifact = Path(sys.argv[1]) if len(sys.argv) > 1 else ROOT / "target/release/kb_count.exe"
    print(json.dumps(verify(artifact.resolve()), indent=2))
