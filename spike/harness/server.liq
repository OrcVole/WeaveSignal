# Spike 01 listener server: Liquidsoap 2.5's icecast.server in its own process.
# In one process with the SRT input, output.icecast to icecast.server stalled (ticks of 0.73 s), so they are split.
settings.log.level := 3
settings.log.stdout := true
settings.harbor.bind_addrs := ["127.0.0.1"]
icecast.server(port=8010, password="spike-only", dedicated_encoder=true)
