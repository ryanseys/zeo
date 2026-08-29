# `#ungetc` pushed a byte onto a stack only `#getbyte` knew about, so
# `f.ungetc("Z"); f.getc` answered the stream's next character and the pushed
# byte was simply LOST -- the exact failure the comment on `peeked_read`
# warns about ("a per-row unget stack would only have been drained by the one
# row that knows about it").
#
# The bytes go into the read buffer now, at the cursor, which every reader
# already consults. Two rules ride with them: the descriptor was never
# advanced past a pushed byte, so giving the read-ahead back must not seek
# over it; and a seek DISCARDS it, because there is nowhere to give it back.
#
# `#pos` was wrong for the same reason -- it reported the raw descriptor
# offset, which sits wherever the last buffered fill stopped.

require "tmpdir"
dir = Dir.mktmpdir
path = File.join(dir, "a")
File.write(path, "one\ntwo\nthree\n")

def fresh(path) = File.open(path)

def show(label)
  puts "#{label} #{yield.inspect}"
rescue Exception => e
  puts "#{label} #{e.class}: #{e.message}"
end

show("getc") { f = fresh(path); f.getc; f.ungetc("Z"); v = f.getc; f.close; v }
show("read") { f = fresh(path); f.getc; f.ungetc("Z"); v = f.read(4); f.close; v }
show("gets") { f = fresh(path); f.getc; f.ungetc("Z"); v = f.gets; f.close; v }
show("getbyte") { f = fresh(path); f.getc; f.ungetc("Z"); v = f.getbyte; f.close; v }
show("readpartial") { f = fresh(path); f.getc; f.ungetc("Z"); v = f.readpartial(3); f.close; v }
show("read all") { f = fresh(path); f.getc; f.ungetc("Z"); v = f.read; f.close; v }
show("each_char") { f = fresh(path); f.getc; f.ungetc("Z"); v = f.each_char.first(3); f.close; v }

# A multi-byte push reads back in order, and an Integer pushes one byte.
show("two bytes") { f = fresh(path); f.ungetc("AB"); v = [f.getc, f.getc, f.getc]; f.close; v }
show("ungetbyte") { f = fresh(path); f.ungetbyte(65); v = f.getc; f.close; v }

# The position counts what has been HANDED OUT, not what the descriptor read.
show("pos") { f = fresh(path); f.getc; f.ungetc("Z"); v = f.pos; f.close; v }
show("pos after getc") { f = fresh(path); f.getc; v = f.pos; f.close; v }
show("eof?") { f = fresh(path); f.read; f.ungetc("Z"); v = [f.eof?, f.getc]; f.close; v }

# A seek throws the pushed byte away.
show("rewind") { f = fresh(path); f.getc; f.ungetc("Z"); f.rewind; v = f.getc; f.close; v }
show("seek") { f = fresh(path); f.getc; f.ungetc("Z"); f.seek(0); v = f.getc; f.close; v }

# A pipe has no descriptor to give read-ahead back to, and works the same.
show("pipe") { r, w = IO.pipe; w.write("xy"); w.close; r.getc; r.ungetc("Q"); v = r.read; r.close; v }

show("answers nil") { f = fresh(path); v = f.ungetc("Z"); f.close; v }

require "fileutils"
FileUtils.remove_entry(dir)
