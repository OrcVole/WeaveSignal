# Spike 01 playout: a fixed script (fifth synthesis, D48). The programme arrives from the desk as
# Matroska over SRT on loopback (D49) and goes to icecast.server, which runs as a separate process (server.liq).
settings.log.level := 3
settings.log.stdout := true
prog = input.srt(bind_address="127.0.0.1", port=9000, mode="listener")
# Not mksafe: it starts on silence and only switches at a track boundary, which silence never reaches.
# Liquidsoap 2.5 sets track_sensitive per child, not on fallback itself.
prog = fallback([prog.{track_sensitive = {false}}, blank().{track_sensitive = {false}}])
output.icecast(%ogg(%opus(bitrate=96, channels=2)), host="127.0.0.1", port=8010, password="spike-only", mount="programme.opus", prog)
