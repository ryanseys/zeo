# IO handle values: #class discrimination, sync, write capture, flush self,
# close nil, << through an alias, and $stdout as a first-class IO value.
File.write("/tmp/sp_io1.txt", "hi")
File.open("/tmp/sp_io1.txt") { |f| p f.fileno.class }
File.open("/tmp/sp_io1.txt") { |f| p f.class }
p $stdout.class
p STDOUT.sync
File.open("/tmp/sp_io1.txt", "w") do |f|
  n = f.write("hi")
  p n
  p f.flush.equal?(f)
end
f = File.open("/tmp/sp_io1.txt")
p f.close
File.open("/tmp/sp_io1.txt", "w") do |f|
  h = f
  h << "a"
  h << "b"
end
p File.read("/tmp/sp_io1.txt")
File.delete("/tmp/sp_io1.txt")
