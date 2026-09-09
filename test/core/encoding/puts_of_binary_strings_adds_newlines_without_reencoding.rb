# The second argument already ends in a newline BYTE -- `puts` must
# not double it, and the 0xB5 byte must survive un-promoted.
require "tmpdir"
ZTMP = Dir.mktmpdir


path = File.join(ZTMP, "zeo_e2e_binputs_#{Process.pid}")
File.open(path, "wb") { |f| f.puts 180.chr, (181.chr + "\n") }
p File.binread(path).bytes
File.delete(path)
__END__
[180, 10, 181, 10]
