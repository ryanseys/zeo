r, w = IO.pipe
puts r.class
puts w.class
w.write("hello")
w.close
puts r.read
