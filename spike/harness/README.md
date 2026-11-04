# Spike 01 harness

Throwaway code for the live-room spike. The plan, method and results are in the WeaveSignal project folder,
`research/2026-10-24-spike-01-live-room.md`. Nothing here is product code.

- `livekit.yaml`: LiveKit 1.13.10, loopback only, development keys.
- `playout.liq`: Liquidsoap 2.5.0-rc1, SRT in on loopback, `icecast.server` out.
- `desk_spike.py`: the desk and the measuring participants in one GStreamer pipeline (Python by exception:
  a measurement harness, not the desk; the desk proper is gstreamer-rs).
