path = "/tmp/zeo_e2e_binshovel_#{Process.pid}"
File.open(path, "wb") { |f| f << 180.chr << "a" }
p File.binread(path).bytes
File.delete(path)
p STDOUT.write("")
__END__
[180, 97]
0
