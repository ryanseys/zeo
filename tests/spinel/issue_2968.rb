d = "/tmp/sp_dir_2968_#{Process.pid}"
Dir.mkdir(d) unless Dir.exist?(d)
dd = Dir.new(d)
r = ((dd.pos = 0) rescue $!.class); p r
dd.close
Dir.rmdir(d)
