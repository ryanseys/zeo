d = "/tmp/sp_dir_2967_#{Process.pid}"
Dir.mkdir(d) unless Dir.exist?(d)
dd = Dir.new(d)
p (dd.seek(0).class rescue $!.class)
p (dd.fileno.class rescue $!.class)
dd.close
Dir.rmdir(d)
