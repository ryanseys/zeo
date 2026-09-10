# Each answers, and pread fills the buffer it was given.
require "tmpdir"
ZTMP = Dir.mktmpdir

p001 = File.join(ZTMP, "sp_io_3131.tmp")
File.write(p001, "hi")
File.open(p001) { |f| f.binmode; p f.binmode? }
File.open(p001) { |f| f.autoclose = false; p f.autoclose? }
File.open(p001) { |f| buf = +""; f.pread(2, 0, buf); p buf }
File.write(File.join(ZTMP, "sp_io_3131b.tmp"), "other")
File.open(p001) { |f| File.open(File.join(ZTMP, "sp_io_3131b.tmp")) { |g| f.reopen(g); p f.read } }
File.delete(p001); File.delete(File.join(ZTMP, "sp_io_3131b.tmp"))
__END__
true
false
"hi"
"other"
