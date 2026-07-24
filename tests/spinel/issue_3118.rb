p001 = "/tmp/sp_mkfifo_3118_#{Process.pid}"
File.delete(p001) if File.exist?(p001)
r = begin; File.mkfifo(p001); rescue => e; e.class; end
p r
p File.exist?(p001)
File.delete(p001)
p File.exist?(p001)
p File.exist?("/tmp")
