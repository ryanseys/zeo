# It exists afterwards, and the call answers rather than raising.
# (spinel issue #3118)
require "tmpdir"
ZTMP = Dir.mktmpdir

p001 = File.join(ZTMP, "sp_mkfifo_3118_#{Process.pid}")
File.delete(p001) if File.exist?(p001)
r = begin; File.mkfifo(p001); rescue => e; e.class; end
p r
p File.exist?(p001)
File.delete(p001)
p File.exist?(p001)
p File.exist?("/tmp")
__END__
0
true
false
true
