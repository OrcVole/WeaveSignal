# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# Designs one voice per character of the demo video (tests/demo-video/script.json): OmniVoice's
# voice design mode, which makes a voice from a description (gender, age, pitch, accent) and a
# seed, with no reference recording and so no real person. Each character's design text is read
# in its designed voice and written as <out>/<id>.wav with its transcript in <out>/<id>.txt;
# synth.sh then reads every line through VoiceLab's tts-serve with that clip as the reference
# and the same seed, so a character keeps one voice across all their lines (tts-serve's
# OmniVoice server has no voice-design field of its own).
#
# Run through design.sh, which points Mojo at the VoiceLab OmniVoice environment. Only one
# OmniVoice may hold the GPU at a time: run this with voicelab-tts stopped.
#
#   tests/demo-video/design.sh [seed-offset]
from std.python import Python, PythonObject
from std.sys import argv


def main() raises:
    var args = argv()
    if len(args) < 3:
        raise Error("usage: design.mojo <script.json> <out-dir> [seed-offset]")
    var script_path = String(args[1])
    var out_dir = String(args[2])
    var offset = 0
    if len(args) > 3:
        offset = Int(String(args[3]))

    # Mojo's own launcher is a user-site Python program: keep that site's packages out of
    # this interpreter, so only the OmniVoice environment's are imported.
    var sys = Python.import_module("sys")
    var kept = Python.list()
    for entry in sys.path:
        if not String(entry).__contains__("/.local/lib/python"):
            kept.append(entry)
    sys.path = kept
    var json = Python.import_module("json")
    var os = Python.import_module("os")
    var random = Python.import_module("random")
    var np = Python.import_module("numpy")
    var torch = Python.import_module("torch")
    var sf = Python.import_module("soundfile")
    var builtins = Python.import_module("builtins")
    var omnivoice = Python.import_module("omnivoice")

    var script = json.load(builtins.open(script_path))
    var cast = script["cast"]
    var text = script["design_text"]
    os.makedirs(out_dir, exist_ok=True)

    print("loading OmniVoice")
    var model = omnivoice.OmniVoice.from_pretrained(
        "k2-fsa/OmniVoice", device_map="cuda", torch_dtype=torch.bfloat16
    )
    var rate = model.sampling_rate
    for key in builtins.list(cast.keys()):
        var who = cast[key]
        var seed = Int(py=who["seed"]) + offset
        random.seed(seed)
        np.random.seed(seed)
        torch.manual_seed(seed)
        torch.cuda.manual_seed_all(seed)
        var audios = model.generate(
            text=text, language="en", instruct=who["voice"]
        )
        var wav = String(out_dir, "/", String(key), ".wav")
        sf.write(wav, audios[0], rate)
        var f = builtins.open(String(out_dir, "/", String(key), ".txt"), "w")
        _ = f.write(text)
        _ = f.close()
        print(String(key), seed, String(who["voice"]), "->", wav)
