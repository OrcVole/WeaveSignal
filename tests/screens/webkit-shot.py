#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# Render a page in WebKitGTK 4.1, the engine the studio app's Tauri window uses on Linux, and
# write a PNG of it: the render spike's captures (ADR-0021) when the window itself cannot be
# captured on the desktop. The view is created at the size asked for, the page is loaded, a
# moment is given for the calendar to lay out, and the view's snapshot is written. Nothing is
# shown on screen (an offscreen GTK window).
#
#   python3 tests/screens/webkit-shot.py URL OUT.png [WIDTH HEIGHT [SETTLE_MS]]
import sys
import gi

gi.require_version("Gtk", "3.0")
gi.require_version("WebKit2", "4.1")
from gi.repository import GLib, Gtk, WebKit2  # noqa: E402

url, out = sys.argv[1], sys.argv[2]
width = int(sys.argv[3]) if len(sys.argv) > 3 else 1440
height = int(sys.argv[4]) if len(sys.argv) > 4 else 900
settle = int(sys.argv[5]) if len(sys.argv) > 5 else 2500

window = Gtk.OffscreenWindow()
window.set_default_size(width, height)
view = WebKit2.WebView()
view.set_size_request(width, height)
window.add(view)
window.show_all()
status = {"code": 0}


def saved(view, result):
    try:
        surface = view.get_snapshot_finish(result)
        surface.write_to_png(out)
        print(f"wrote {out} ({surface.get_width()} x {surface.get_height()})")
    except Exception as e:  # noqa: BLE001
        print(f"snapshot failed: {e}", file=sys.stderr)
        status["code"] = 1
    Gtk.main_quit()


def snap():
    view.get_snapshot(WebKit2.SnapshotRegion.VISIBLE, WebKit2.SnapshotOptions.NONE, None, saved)
    return False


def loaded(view, event):
    if event == WebKit2.LoadEvent.FINISHED:
        GLib.timeout_add(settle, snap)


def failed(view, event, uri, error):
    print(f"load failed: {uri}: {error}", file=sys.stderr)
    status["code"] = 1
    Gtk.main_quit()
    return True


view.connect("load-changed", loaded)
view.connect("load-failed", failed)
view.load_uri(url)
GLib.timeout_add(settle + 30000, lambda: (print("timed out", file=sys.stderr), Gtk.main_quit()) and False)
Gtk.main()
sys.exit(status["code"])
