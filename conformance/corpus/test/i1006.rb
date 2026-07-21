path = "i1006_tmp.txt"
File.delete(path) if File.exist?(path)

f = File.open(path, "w")
f.write("hello\n")
f.write("world\n")
f.close
puts f.closed?

g = File.open(path, "r")
puts g.read
puts g.eof?
g.close

h = File.open(path, "r")
puts h.gets.chomp
puts h.gets.chomp
puts h.gets.nil?
h.close

File.delete(path)
