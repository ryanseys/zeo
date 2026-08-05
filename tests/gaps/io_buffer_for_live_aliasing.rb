# `IO::Buffer.for(string) { }` really ALIASES the string's bytes in CRuby:
# a buffer write is visible in the string BEFORE the block ends (the string
# is locked against its own mutation for the duration, but reads see the
# buffer's writes live). zeo strings live behind an Arc<Mutex<StrBuf>> (no
# stable byte address), so the block form copies in and copies back at exit
# -- the END state matches CRuby, but the mid-block read sees the original.
# See docs/COMPATIBILITY.md.
$stderr.reopen(IO::NULL)
s = +"abcdef"
IO::Buffer.for(s) do |b|
  b.set_value(:U8, 0, 0x5a)
  p s
end
p s
