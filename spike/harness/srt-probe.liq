# Does input.srt release audio while the stream is live? Prints readiness and buffer once a second.
settings.log.level := 2
settings.log.stdout := true
s = input.srt(bind_address="127.0.0.1", port=9001, mode="listener")
thread.run(every=1., fun () -> print("t=#{time()} ready=#{s.is_ready()} buffered=#{s.buffered()}"))
output.dummy(fallible=true, s)
