# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# Reads back the QR codes tests/keys/qr-check.mjs drew (and any picture named after them) with
# OpenCV's decoder, which shares no code with the pages' encoder, and says which came back as
# written. Run it through uv, so nothing is installed in the repository:
#
#   uvx --with opencv-python-headless python tests/keys/qr-decode.py target/qr-check
#   uvx --with opencv-python-headless python tests/keys/qr-decode.py --picture shot.png "the text it should say"
import json
import sys
from pathlib import Path

import cv2


def read(path: Path) -> str:
    img = cv2.imread(str(path), cv2.IMREAD_GRAYSCALE)
    if img is None:
        return ""
    det = cv2.QRCodeDetector()
    for scale in (1, 2, 3):
        big = img if scale == 1 else cv2.resize(img, None, fx=scale, fy=scale, interpolation=cv2.INTER_NEAREST if path.suffix == ".pbm" else cv2.INTER_CUBIC)
        text, _points, _straight = det.detectAndDecode(big)
        if text:
            return text
    return ""


def main() -> int:
    if len(sys.argv) >= 4 and sys.argv[1] == "--picture":
        got = read(Path(sys.argv[2]))
        ok = got == sys.argv[3]
        print("PASS" if ok else "FAIL", sys.argv[2], "" if ok else f"read {got!r}")
        return 0 if ok else 1
    folder = Path(sys.argv[1] if len(sys.argv) > 1 else "target/qr-check")
    expected = json.loads((folder / "expected.json").read_text())
    failed = 0
    for e in expected:
        got = read(folder / e["file"])
        ok = got == e["text"]
        failed += 0 if ok else 1
        print("PASS" if ok else "FAIL", e["file"], f"version {e['version']} mask {e['mask']}", "" if ok else f"read {got[:60]!r}")
    print(f"{len(expected) - failed} of {len(expected)} codes read back as written")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
