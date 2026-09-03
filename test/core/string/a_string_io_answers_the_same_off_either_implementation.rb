# stringio -- the differential matrix all three implementations answer:
# ruby's C gem (the recorded text), zeo's native ext, and the pure port.
require "stringio"

def t(label)
  print label, ": "
  p yield
rescue => e
  puts "#{e.class}: #{e.message}"
end

s = StringIO.new("line one\nline two\nline three\n")
t("class") { s.class }
t("read4") { s.read(4) }
t("pos") { s.pos }
t("gets") { s.gets }
t("lineno") { s.lineno }
t("gets_chomp") { s.gets(chomp: true) }
t("gets_limit") { s.gets(3) }
t("gets_sep") { s.gets("ee") }
t("rest") { s.read }
t("eof") { s.eof? }
t("read_at_eof") { s.read }
t("read4_at_eof") { s.read(4) }
t("rewind") { [s.rewind, s.pos, s.lineno] }
t("each_line") { ls = []; s.each_line { |l| ls << l }; ls }
t("readlines") { s.rewind; s.readlines(chomp: true) }

# Bytes and characters.
b = StringIO.new("héllo")
t("getc") { b.getc }
t("getc2") { b.getc }
t("getbyte") { b.getbyte }
t("ungetbyte") { b.ungetbyte(0x41); b.read }
t("ungetc_head") { b.rewind; b.ungetc("XY"); b.read }
t("each_char") { b.rewind; b.each_char.to_a }
t("each_byte_n") { b.rewind; b.each_byte.to_a.length }
t("each_codepoint") { b.rewind; b.each_codepoint.to_a }
t("readchar_eof") { b.read; b.readchar }
t("readbyte_eof") { b.readbyte }
t("getbyte_eof") { b.getbyte }

# Writing.
w = StringIO.new
t("write") { w.write("ab", "cd") }
t("putc") { w.putc(0x65) }
t("putc_str") { w.putc("fgh") }
t("print") { w.print("i", "j") }
t("puts") { w.puts("k", ["l", "m"]) }
t("printf") { w.printf("%04d!", 42) }
t("shovel") { (w << "z" << 9).string }
t("size") { w.size }
t("sync") { w.sync }

# Position games: seek, overwrite mid-string, past-end zero fill.
o = StringIO.new(+"abcdef")
t("seek") { [o.seek(2), o.pos] }
t("overwrite") { o.write("XY"); o.string }
t("seek_end") { [o.seek(-1, IO::SEEK_END), o.read] }
t("seek_cur") { o.seek(2); [o.seek(1, IO::SEEK_CUR), o.pos] }
t("past_end") { o.pos = 9; o.write("!"); o.string }
t("seek_neg") { o.seek(-1) }
t("truncate") { [o.truncate(3), o.string] }
t("truncate_grow") { [o.truncate(6), o.string.bytes] }
t("truncate_neg") { o.truncate(-2) }

# Append and read-write modes; mode errors.
t("append") { a = StringIO.new(+"ab", "a"); a << "c"; a.string }
t("append_seek") { a = StringIO.new(+"ab", "a"); a.rewind; a.write("Z"); a.string }
t("readonly_write") { StringIO.new("x", "r").write("y") }
t("writeonly_read") { StringIO.new(+"x", "w").read }
t("w_truncates") { StringIO.new(+"long", "w").string }
t("bad_mode") { StringIO.new(+"x", "q") }
t("frozen_write") { StringIO.new("frozen".freeze).write("y") }
t("frozen_mode_w") { StringIO.new("frozen".freeze, "w") }

# The buffer is SHARED, never a copy.
sh = +"shared"
shio = StringIO.new(sh)
t("shared_read") { shio.read(2) }
t("shared_mutate") { sh[0] = "S"; shio.read }
t("string_eq") { shio.string = +"swapped"; [shio.pos, shio.lineno, shio.read] }

# Paragraph mode, and nil separators.
para = StringIO.new("one\n\n\ntwo\nstill two\n\nthree\n")
t("paragraphs") { para.each_line("").to_a }
t("gets_nil") { para.rewind; para.gets(nil) }

# readpartial / read_nonblock / sysread.
r = StringIO.new("abc")
t("readpartial") { r.readpartial(2) }
t("readpartial_buf") { buf = +""; r.readpartial(2, buf); buf }
t("readpartial_eof") { r.readpartial(2) }
t("read_nonblock_eof") { r.read_nonblock(2) }
t("read_nonblock_exc") { r.read_nonblock(2, exception: false) }
t("sysread_eof") { r.sysread(1) }

# Close halves.
c = StringIO.new(+"cl")
t("close_read") { c.close_read; c.read }
t("closed_read?") { [c.closed_read?, c.closed_write?, c.closed?] }
t("close_write") { c.close_write; c.write("x") }
t("closed_all") { [c.closed?, c.string] }
t("reread_closed") { c.gets }
t("close") { c2 = StringIO.new; c2.close; [c2.closed?, (c2.string rescue $!.class)] }

# IO-flavored odds and ends.
i = StringIO.new("x")
t("fileno") { i.fileno }
t("isatty") { i.isatty }
t("pid") { i.pid }
t("flush") { i.flush == i }
t("fsync") { i.fsync }
t("binmode") { [i.binmode == i, i.external_encoding] }
t("internal_enc") { i.internal_encoding }
t("external_enc") { StringIO.new("é").external_encoding }
t("set_encoding") { e = StringIO.new(+"é"); e.set_encoding("binary"); [e.external_encoding, e.read.bytes.length] }
t("open_block") { StringIO.open("blk") { |io| io.read(3) } }
t("marshal") { Marshal.dump(StringIO.new) }
t("each_with_sep_limit") { StringIO.new("aXbXcX").each_line("X", 2).to_a }
t("lineno_tracks_gets") { g = StringIO.new("a\nb\nc\n"); g.gets; g.gets; g.lineno }
t("lineno_eq") { g = StringIO.new("a\nb\n"); g.lineno = 7; g.gets; g.lineno }
t("tell") { g = StringIO.new("abc"); g.read(1); g.tell }
t("pos_eq_neg") { StringIO.new("abc").pos = -1 }
t("inspect_shape") { StringIO.new.inspect.start_with?("#<StringIO:0x") }
__END__
class: StringIO
read4: "line"
pos: 4
gets: " one\n"
lineno: 1
gets_chomp: "line two"
gets_limit: "lin"
gets_sep: "e three"
rest: "\n"
eof: true
read_at_eof: ""
read4_at_eof: nil
rewind: [0, 0, 0]
each_line: ["line one\n", "line two\n", "line three\n"]
readlines: ["line one", "line two", "line three"]
getc: "h"
getc2: "é"
getbyte: 108
ungetbyte: "Alo"
ungetc_head: "XYhéAlo"
each_char: ["X", "Y", "h", "é", "A", "l", "o"]
each_byte_n: 8
each_codepoint: [88, 89, 104, 233, 65, 108, 111]
readchar_eof: EOFError: end of file reached
readbyte_eof: EOFError: end of file reached
getbyte_eof: nil
write: 4
putc: 101
putc_str: "fgh"
print: nil
puts: nil
printf: nil
shovel: "abcdefijk\nl\nm\n0042!z9"
size: 21
sync: true
seek: [0, 2]
overwrite: "abXYef"
seek_end: [0, "f"]
seek_cur: [0, 3]
past_end: "abXYef\u0000\u0000\u0000!"
seek_neg: Errno::EINVAL: Invalid argument
truncate: [0, "abX"]
truncate_grow: [0, [97, 98, 88, 0, 0, 0]]
truncate_neg: Errno::EINVAL: Invalid argument - negative length
append: "abc"
append_seek: "abZ"
readonly_write: IOError: not opened for writing
writeonly_read: IOError: not opened for reading
w_truncates: ""
bad_mode: ArgumentError: invalid access mode q
frozen_write: IOError: not opened for writing
frozen_mode_w: Errno::EACCES: Permission denied
shared_read: "sh"
shared_mutate: "ared"
string_eq: [0, 0, "swapped"]
paragraphs: ["one\n\n\n", "two\nstill two\n\n", "three\n"]
gets_nil: "one\n\n\ntwo\nstill two\n\nthree\n"
readpartial: "ab"
readpartial_buf: "c"
readpartial_eof: EOFError: end of file reached
read_nonblock_eof: EOFError: end of file reached
read_nonblock_exc: nil
sysread_eof: EOFError: end of file reached
close_read: IOError: not opened for reading
closed_read?: [true, false, false]
close_write: IOError: not opened for writing
closed_all: [true, "cl"]
reread_closed: IOError: not opened for reading
close: [true, ""]
fileno: nil
isatty: false
pid: nil
flush: true
fsync: 0
binmode: [true, #<Encoding:BINARY (ASCII-8BIT)>]
internal_enc: nil
external_enc: #<Encoding:UTF-8>
set_encoding: [#<Encoding:BINARY (ASCII-8BIT)>, 2]
open_block: "blk"
marshal: TypeError: no _dump_data is defined for class StringIO
each_with_sep_limit: ["aX", "bX", "cX"]
lineno_tracks_gets: 2
lineno_eq: 8
tell: 1
pos_eq_neg: Errno::EINVAL: Invalid argument
inspect_shape: true
