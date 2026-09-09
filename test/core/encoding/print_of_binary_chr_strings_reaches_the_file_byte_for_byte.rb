require "tmpdir"
ZTMP = Dir.mktmpdir

path = File.join(ZTMP, "zeo_e2e_binprint_#{Process.pid}")
File.open(path, "wb") { |f| f.print 180.chr, 0.chr, 255.chr }
p File.binread(path).bytes
File.delete(path)
__END__
[180, 0, 255]
