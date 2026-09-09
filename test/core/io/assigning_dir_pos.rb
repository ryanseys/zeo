# `dd.pos = 0` answers the position rather than raising.
# (spinel issue #2968)
require "tmpdir"
ZTMP = Dir.mktmpdir

d = File.join(ZTMP, "sp_dir_2968_#{Process.pid}")
Dir.mkdir(d) unless Dir.exist?(d)
dd = Dir.new(d)
r = ((dd.pos = 0) rescue $!.class); p r
dd.close
Dir.rmdir(d)
__END__
0
