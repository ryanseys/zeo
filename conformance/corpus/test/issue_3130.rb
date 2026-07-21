p001 = "sp_io_3130.tmp"
File.write(p001, "hi")
File.open(p001) { |f| r001 = (f.fcntl(1, 0) rescue $!.class); p r001 }
File.delete(p001)
