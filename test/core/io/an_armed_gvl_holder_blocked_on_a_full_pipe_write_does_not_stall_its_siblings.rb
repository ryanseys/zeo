# The write-side twin (the `write_rio` seam): 200KB overflows the
# kernel pipe buffer, so the writer blocks mid-`write_all` until main
# drains the pipe -- which main can only do if the writer released
# the armed Gvl first.
#@ env: ZEO_GVL=1

r, w = IO.pipe
writer = Thread.new { w.write("x" * 200_000); w.close; :done }
sleep 0.2
data = r.read
p writer.value
p data.bytesize
__END__
:done
200000
