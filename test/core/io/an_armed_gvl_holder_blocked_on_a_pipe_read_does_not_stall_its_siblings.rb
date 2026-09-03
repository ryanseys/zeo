# The io-family without_gvl probe, in ZEO_GVL=1 fidelity mode: the
# reader parks on an EMPTY pipe while main sleeps, so without the
# release at the `with_file` seam the reader would block INSIDE the
# Gvl and main could never wake to perform the write -- this test
# hangs there. Completing IS the proof of release.
#@ env: ZEO_GVL=1

r, w = IO.pipe
reader = Thread.new { r.gets }
sleep 0.2
w.puts "hello"
p reader.value
__END__
"hello\n"
