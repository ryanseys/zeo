# each_line re-enters the with_file seam once per line, so the
# consumer parks BETWEEN lines while main wakes to write the next
# one; the closed write end ends the stream.
#@ env: ZEO_GVL=1

r, w = IO.pipe
lines = []
t = Thread.new { r.each_line { |l| lines << l.chomp } }
sleep 0.1
w.puts "alpha"
sleep 0.1
w.puts "beta"
w.close
t.join
p lines
r.close
__END__
["alpha", "beta"]
