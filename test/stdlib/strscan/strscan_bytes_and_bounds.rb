# Two StringScanner rows: `get_byte` answers the RAW byte (zeo converts
# through lossy UTF-8 and answers the replacement character for each
# half of a multibyte char -- data corruption, the to_utf8_lossy
# family), and `pos=` past the end raises RangeError ("index out of
# range"). (Found by the 2026-08-24 probe sweep.)
require "strscan"
s = StringScanner.new("é")
p s.get_byte.bytes
p s.get_byte.bytes
begin
  t = StringScanner.new("ab")
  t.pos = 99
  p t.pos
rescue RangeError => e
  puts e.message
end
__END__
[195]
[169]
index out of range
