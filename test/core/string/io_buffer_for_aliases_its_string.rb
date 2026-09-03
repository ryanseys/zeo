# `IO::Buffer.for(string)` shares the STRING's own bytes, so a buffer write
# is visible through the string before the block ends -- CRuby's buffer
# really aliases its string's storage, and this one does too.
#
# The earlier answer copied in and copied back at block exit: the END state
# matched and a mid-block read of the string saw the original. The recorded
# fix shape was a stable byte address for the string, which zeo's
# `Arc<Mutex<StrBuf>>` does not have and which is a value-model change.
#
# It was not needed. The buffer already held its source string, so the
# backing can BE that string (`Mem::Str`): every read and write locks it and
# uses its bytes in place. The string's own mutex is what CRuby models as
# "the string is locked for the duration", and no address is pinned at all.
# Nothing is copied in either direction now.
$stderr.reopen(IO::NULL)
s = +"abcdef"
IO::Buffer.for(s) do |b|
  b.set_value(:U8, 0, 0x5a)
  p s
  b.set_value(:U8, 5, 0x5a)
  p s
end
p s

# The readonly (no-block) form shares the same bytes, and refuses a write.
r = +"hello"
view = IO::Buffer.for(r)
p view.get_string
p view.readonly?
begin
  view.set_value(:U8, 0, 0x41)
rescue IO::Buffer::AccessError => e
  p [:refused, e.class]
end

# A multi-byte string keeps its encoding across a byte write.
u = +"héllo"
IO::Buffer.for(u) do |b|
  b.set_value(:U8, 0, 0x48)
end
p [u, u.encoding.to_s, u.valid_encoding?]

# `IO::Buffer.new` owns its bytes and aliases nothing.
own = IO::Buffer.new(4)
own.set_value(:U8, 0, 0x41)
p own.get_string
__END__
"Zbcdef"
"ZbcdeZ"
"ZbcdeZ"
"hello"
true
[:refused, IO::Buffer::AccessError]
["Héllo", "UTF-8", true]
"A\x00\x00\x00"
