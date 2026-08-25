# `read` (to EOF) and `eof?` on a PIPE must not seek: with the write end
# closed, ruby answers "" and true. zeo's eof probe seeks and raises
# Errno::ESPIPE ("Illegal seek"). (Found by the 2026-08-24 probe sweep.)
r, w = IO.pipe
w.close
p r.read
p r.eof?
r.close
