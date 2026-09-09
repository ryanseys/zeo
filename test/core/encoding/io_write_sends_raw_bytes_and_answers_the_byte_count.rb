# 1 binary byte + 2 UTF-8 bytes for "é" + 2 display bytes for 42 = 5;
# the count is BYTES, not characters.
require "tmpdir"
ZTMP = Dir.mktmpdir


path = File.join(ZTMP, "zeo_e2e_binwrite_#{Process.pid}")
File.open(path, "wb") { |f| p f.write(180.chr, "é", 42) }
p File.binread(path).bytes
File.delete(path)
__END__
5
[180, 195, 169, 52, 50]
