# `IO::Buffer.for(string) { }` really ALIASES the string's bytes in CRuby:
# a buffer write is visible in the string BEFORE the block ends (the string
# is locked against its own mutation for the duration, but reads see the
# buffer's writes live). zeo strings live behind an Arc<Mutex<StrBuf>> (no
# stable byte address), so the block form copies in and copies back at exit
# -- the END state matches CRuby, but the mid-block read sees the original.
# See docs/COMPATIBILITY.md.
#
# The divergence window is exactly the block's own duration, and only for a
# read of the STRING (reads through the buffer are live in both). Nothing that
# writes and then checks the buffer diverges; what diverges is code that hands
# the same string to something else mid-block, which is rare enough that the
# copy-in/copy-back was the accepted trade.
#
# A fix is a string representation with a stable byte address for the
# duration -- pinning the `StrBuf`'s allocation and handing the buffer a raw
# pointer into it, with the existing mutation lock preventing a reallocation
# underneath. That is a value-model change, not an IO::Buffer one, which is
# why it sits here rather than in the buffer's own surface. `IO::Buffer.new`
# (an OWNED buffer, no aliasing promised) is unaffected either way.
$stderr.reopen(IO::NULL)
s = +"abcdef"
IO::Buffer.for(s) do |b|
  b.set_value(:U8, 0, 0x5a)
  p s
end
p s
