# The with_file seam beyond gets: readline and a LENGTHED read(n)
# each park on the pipe in their own Gvl-released section, with main
# feeding the pipe in stages between sleeps -- two consecutive
# blocking reads on one fd, each of which must release.
#@ env: ZEO_GVL=1

r, w = IO.pipe
consumer = Thread.new do
  first = r.readline
  rest = r.read(4)
  [first, rest]
end
sleep 0.2
w.puts "line"
sleep 0.1
w.write "tail"
first, rest = consumer.value
p first
p rest
r.close
w.close
__END__
"line\n"
"tail"
