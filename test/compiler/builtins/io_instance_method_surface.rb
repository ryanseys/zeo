# ungetbyte pushes a byte back for the next read; binmode/binmode?,
# autoclose=/autoclose?, to_io (self), close_on_exec?, pread/pwrite at a
# fixed offset, advise (nil), close_write on a read-only file (IOError),
# reopen (rebinds to another file), each_codepoint.
require "tmpdir"
ZTMP = Dir.mktmpdir


pth = File.join(ZTMP, "sp_e2e_io_#{Process.pid}.tmp")
File.write(pth, "hi\n")
File.open(pth) do |f|
  p f.readbyte
  f.ungetbyte(104)
  p f.readbyte
  p f.binmode?
  f.binmode
  p f.binmode?
  p f.to_io.equal?(f)
  p f.close_on_exec?
  p f.pread(2, 0)
  p f.advise(:normal)
  p((f.autoclose = false))
  p f.autoclose?
  r = (begin; f.close_write; rescue => e; e.class; end); p r
end
File.open(pth, "r+") { |f| p f.pwrite("X", 0) }
p File.read(pth)
File.write(File.join(ZTMP, "sp_e2e_io2_#{Process.pid}.tmp"), "other")
File.open(pth) { |f| File.open(File.join(ZTMP, "sp_e2e_io2_#{Process.pid}.tmp")) { |g| f.reopen(g); p f.read } }
File.open(pth) { |f| cps = []; f.each_codepoint { |c| cps << c }; p cps }
File.delete(pth); File.delete(File.join(ZTMP, "sp_e2e_io2_#{Process.pid}.tmp"))
__END__
104
104
false
true
true
true
"hi"
nil
false
false
IOError
1
"Xi\n"
"other"
[88, 105, 10]
