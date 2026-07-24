File.write("tfg.tmp", "line1\nline2\n")
f = File.open("tfg.tmp")
t = Thread.new do
  a = f.gets
  b = f.gets
  [a, b]
end
p t.value
f.close
File.delete("tfg.tmp")
