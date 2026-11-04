# weavesignal-desk-spike image

A throwaway image for the WeaveSignal live-room spike. The "desk" is a GStreamer pipeline that
joins a LiveKit room, mixes the participants and sends Matroska over SRT to Liquidsoap.

## What it builds

A multi-stage `Containerfile` on `fedora:44`:

1. **Builder stage.** This stage builds two plugins from source:
   - `libgstsrt.so` (`srtsrc`, `srtsink`). It comes from the upstream
     `gst-plugins-bad-1.28.8.tar.xz`, with its sha256 pinned, and is built with meson with every
     feature off except `srt`, against Fedora's `srt-devel` 1.5.7. Fedora's `bad-free` package
     leaves SRT out, and so does RPM Fusion's `bad-freeworld`. RPM Fusion is not used.
   - `libgstrswebrtc.so`. It comes from gst-plugins-rs tag **`0.15.4+fixup`**, the 0.15
     series built for GStreamer 1.28 (`+fixup` changes only `Cargo.lock`). It is built with
     `cargo cinstall --locked -p gst-plugin-webrtc --features livekit` on Fedora's Rust 1.99
     and cargo-c 0.10.25. The default features are kept, which gives `whipclientsink`, WHEP
     and Janus as well as LiveKit.
2. **Runtime stage.** Fedora's GStreamer 1.28.8 packages: base, good, bad-free (which includes
   `webrtcbin` and `nvcodec`), the base tools, and `libnice-gstreamer1`. Also `srt-libs`,
   `opus`, and Python 3.14 with `python3-gobject`, `python3-gstreamer1` and `numpy`. The two
   plugins above are copied in.

During the build, `gst-inspect-1.0 --exists` checks every required element and the Python
`Gst` import is tried. The GStreamer registry cache is then deleted, so a run with the GPU
scans `nvcodec` again.

There is no maintained prebuilt image to use instead. The upstream CI images
(`registry.freedesktop.org/gstreamer/gstreamer-rs/debian/trixie-slim`) are Rust build
environments without compiled plugins. Fedora 44 does not package gst-plugins-rs.

## Build and run

```sh
cd spike/desk-image   # from the repository's root
podman build -t localhost/weavesignal-desk-spike:dev .
podman run --rm localhost/weavesignal-desk-spike:dev gst-inspect-1.0 livekitwebrtcsrc
# NVIDIA (CDI); Bazzite's SELinux blocks /dev/nvidia* unless labelling is disabled:
podman run --rm --device nvidia.com/gpu=all --security-opt label=disable \
  localhost/weavesignal-desk-spike:dev gst-inspect-1.0 nvav1enc
```

## Result (2026-10-24)

- **Build time:** 4 min 26 s, measured on a cold run (32 cores). Of that, the Rust compile took
  1 min 56 s. The cargo registry and `target/` are cache mounts, so a rebuild is faster.
- **Image size:** about 1.0 GB.
- **Versions:** GStreamer 1.28.8, gst-plugins-rs 0.15.4 (`c164a66`), libsrt 1.5.7,
  Python 3.14.8, PyGObject 3.56.3, numpy 2.4.6.
- **`gst-inspect-1.0`:** these elements pass: `livekitwebrtcsrc`, `livekitwebrtcsink`,
  `whipclientsink`, `srtsink`, `srtsrc`, `matroskamux`, `audiomixer`, `opusenc`, `opusdec`,
  `webrtcbin`, `audiotestsrc` and `level`.
- **Python:** `Gst.ElementFactory.find('livekitwebrtcsrc')` returns a factory.
- **`nvav1enc`:**
  - Without a GPU it is absent, which is expected.
  - With `--device nvidia.com/gpu=all` alone, CUDA fails to initialise because SELinux denies
    access to `/dev/nvidia*`.
  - With `--security-opt label=disable` added, it is present, along with `nvautogpuav1enc`,
    H.264/H.265 encoders and NVDEC. A 4-second 480p30 AV1 encode at 800 kbit/s into
    Matroska worked.
