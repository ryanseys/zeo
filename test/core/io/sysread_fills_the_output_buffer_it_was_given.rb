# It answers that same object, so the caller can read the bytes back out of it.
# (spinel issue #3336)
require "tmpdir"
ZTMP = Dir.mktmpdir

path = File.join(ZTMP, "sp_outbuf_ignored.txt")
File.write(path, "hello world")
File.open(path) { |f| b = +""; r = f.sysread(3, b); p r; p b; p r.equal?(b) }
File.open(path) { |f| b = +""; r = f.readpartial(3, b); p r; p b; p r.equal?(b) }
File.delete(path)
__END__
"hel"
"hel"
true
"hel"
"hel"
true
