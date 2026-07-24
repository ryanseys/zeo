# IO#read(n) reads up to n bytes from the current position, advancing it,
# and returns nil at EOF. The no-arg form reads the rest of the file.
path = "file_read_count_tmp.dat"
File.delete(path) if File.exist?(path)

File.open(path, "w") { |f| f.print "ABCDEFGHIJ" }

f = File.open(path, "r")
puts f.read(5).inspect
puts f.read(5).inspect
puts f.read(5).inspect
f.close

g = File.open(path, "r")
puts g.read(3).inspect
puts g.read.inspect
g.close

File.delete(path) if File.exist?(path)
