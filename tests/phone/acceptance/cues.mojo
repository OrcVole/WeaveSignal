# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# The phone line's acceptance run (17f), the operator's side: shows the cues of script.json in
# time, so the operator can call in, press a key, answer Richard's questions and keep silent on
# cue without hearing the phone. Richard's questions appear as text at the moment the AI voice
# asks them; the harness speaks them from the same file and the same start (T0).
#
#   mojo run tests/phone/acceptance/cues.mojo tests/phone/acceptance/script.json \
#       [route] [t0-epoch-seconds] [speed]
#
# route is `trunk` or `browser` (cues for the other route are left out); t0 is the shared start
# the harness was given (default: 10 s from now, printed); speed > 1 runs the clock faster, for
# a dry run of the schedule only.
from std.python import Python, PythonObject
from std.sys import argv


def main() raises:
    var args = argv()
    if len(args) < 2:
        raise Error("usage: cues.mojo <script.json> [trunk|browser] [t0] [speed]")
    var json = Python.import_module("json")
    var time = Python.import_module("time")
    var builtins = Python.import_module("builtins")
    var route = String("trunk")
    if len(args) > 2:
        route = String(args[2])
    if route != "trunk" and route != "browser":
        raise Error("route is trunk or browser")
    var t0: Float64 = Float64(py=time.time()) + 10.0
    if len(args) > 3:
        t0 = Float64(String(args[3]))
    var speed: Float64 = 1.0
    if len(args) > 4:
        speed = Float64(String(args[4]))

    var script = json.load(builtins.open(String(args[1])))
    var cues = Python.list()
    for c in script["cues"]:
        var who = String(c["who"])
        if who != "operator" and who != "richard":
            continue
        if c.__contains__("route") and String(c["route"]) != route:
            continue
        cues.append(c)
    print(String(script["title"]))
    print("Route:", route, "-", String(script["routes"][route]))
    print("Start (T0):", time.strftime("%H:%M:%S", time.localtime(t0)), "local time")
    print("")

    for c in cues:
        var at = Float64(py=c["at"])
        if String(c["who"]) == "richard":
            var due_r = t0 + at / speed
            var now_r = Float64(py=time.time())
            if due_r > now_r:
                time.sleep(due_r - now_r)
            print("")
            print("    t=" + String(c["at"]) + "s  Richard (" + String(c["role"]) + "): " + String(c["say"]))
            continue
        var label = String(c["do"]).upper()
        var prompt = String("")
        if c.__contains__("prompt"):
            prompt = String(c["prompt"])
        # Warn five seconds ahead (when the clock has room), then show the cue at its second.
        var warn_at = t0 + (at - 5.0) / speed
        var now = Float64(py=time.time())
        if warn_at > now:
            time.sleep((warn_at - now))
            print("   next in 5 s:", label, "-", String(c["text"]))
        now = Float64(py=time.time())
        var due = t0 + at / speed
        if due > now:
            time.sleep(due - now)
        var until = String("")
        if c.__contains__("until"):
            until = String(" (until t=") + String(c["until"]) + "s)"
        print("")
        print(">>> t=" + String(c["at"]) + "s  " + label + until)
        if prompt != "":
            print("    Richard asks: " + prompt)
        print("    " + String(c["text"]))
    print("")
    print("End of the run.")
