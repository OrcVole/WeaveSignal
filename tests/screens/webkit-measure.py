#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# Load a page in WebKitGTK 4.1, the engine the studio app's Tauri window uses on Linux, run a
# script in it, print what the script returns (JSON) and write a PNG of the view. For layout
# checks that must hold in the app's own engine and not only in Chromium: `zoom` on <html> (the
# studio's text size) and container queries are where the two have differed. Nothing is shown
# on screen (an offscreen GTK window).
#
#   python3 tests/screens/webkit-measure.py URL OUT.png WIDTH HEIGHT INIT.js MEASURE.js [SETTLE_MS]
#
# INIT.js runs before the page's own scripts (set localStorage there). MEASURE.js is run twice:
# once after the page has loaded with `window.__step = 'open'` (go to the screen wanted), then
# after SETTLE_MS with `window.__step = 'measure'`: what that second run evaluates to, a JSON
# string, is printed on one line starting `RESULT `.
import sys
import gi

gi.require_version("Gtk", "3.0")
gi.require_version("WebKit2", "4.1")
from gi.repository import GLib, Gtk, WebKit2  # noqa: E402

url, out = sys.argv[1], sys.argv[2]
width, height = int(sys.argv[3]), int(sys.argv[4])
init_js = open(sys.argv[5], encoding="utf-8").read()
measure_js = open(sys.argv[6], encoding="utf-8").read()
settle = int(sys.argv[7]) if len(sys.argv) > 7 else 2500

manager = WebKit2.UserContentManager()
manager.add_script(
    WebKit2.UserScript(
        init_js,
        WebKit2.UserContentInjectedFrames.TOP_FRAME,
        WebKit2.UserScriptInjectionTime.START,
        None,
        None,
    )
)
window = Gtk.OffscreenWindow()
window.set_default_size(width, height)
view = WebKit2.WebView.new_with_user_content_manager(manager)
view.set_size_request(width, height)
window.add(view)
window.show_all()
status = {"code": 0, "opened": False}


def fail(why):
    print(why, file=sys.stderr)
    status["code"] = 1
    Gtk.main_quit()


def saved(view, result):
    try:
        surface = view.get_snapshot_finish(result)
        surface.write_to_png(out)
    except Exception as e:  # noqa: BLE001
        return fail(f"snapshot failed: {e}")
    Gtk.main_quit()


def measured(view, result):
    try:
        value = view.evaluate_javascript_finish(result)
        print("RESULT " + value.to_string(), flush=True)
    except Exception as e:  # noqa: BLE001
        return fail(f"measure failed: {e}")
    view.get_snapshot(WebKit2.SnapshotRegion.VISIBLE, WebKit2.SnapshotOptions.NONE, None, saved)


def measure():
    view.evaluate_javascript("window.__step = 'measure';\n" + measure_js, -1, None, None, None, measured)
    return False


def opened(view, result):
    try:
        view.evaluate_javascript_finish(result)
    except Exception as e:  # noqa: BLE001
        return fail(f"open failed: {e}")
    GLib.timeout_add(settle, measure)


def open_screen():
    view.evaluate_javascript("window.__step = 'open';\n" + measure_js, -1, None, None, None, opened)
    return False


def loaded(view, event):
    if event == WebKit2.LoadEvent.FINISHED and not status["opened"]:
        status["opened"] = True
        GLib.timeout_add(settle, open_screen)


def failed(view, event, uri, error):
    fail(f"load failed: {uri}: {error}")
    return True


view.connect("load-changed", loaded)
view.connect("load-failed", failed)
GLib.timeout_add(60000, lambda: fail("timed out"))
view.load_uri(url)
Gtk.main()
sys.exit(status["code"])
