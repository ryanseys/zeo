# `IO.new(fd, mode, **opts)` -- the same mode and the same options `File.open`
# takes, and they mean the same things.
#
# zeo's row was `(fd, mode?)`: the mode was READ and then dropped, no option was
# ever looked at, and a third argument was `wrong number of arguments (given 3,
# expected 1..2)`. So a handle over a descriptor always reported UTF-8 whatever
# it was asked for, and irb's
#
#   IO.open(STDOUT.to_i, 'w', external_encoding: enc, internal_encoding: "-")
#
# could not be built at all -- which is where `IRB.start` died.
require "tmpdir"
require "fileutils"

DIR = Dir.mktmpdir("zeo_io_open_opts")
SRC = File.join(DIR, "src.txt")
File.binwrite(SRC, "hello\nworld\n")

# Every row gets a descriptor of its own, and whatever the row builds owns it:
# closing one fd twice is a real error, not a test detail.
def row(name, mode = "r")
  fd = IO.sysopen(SRC, mode)
  made = nil
  begin
    value, made = yield fd
    puts "#{name}\t#{value.inspect}"
  rescue StandardError => e
    puts "#{name}\t#{e.class}: #{e.message}"
  ensure
    if made
      made.close
    else
      begin
        IO.for_fd(fd).close
      rescue SystemCallError
        nil
      end
    end
  end
end

def encs(io)
  [[io.external_encoding&.name, io.internal_encoding&.name], io]
end

# --- the mode argument -------------------------------------------------------
row("no mode")            { |fd| encs(IO.new(fd)) }
row("mode r")             { |fd| encs(IO.new(fd, "r")) }
row("mode rb")            { |fd| encs(IO.new(fd, "rb")) }
row("mode r:enc")         { |fd| encs(IO.new(fd, "r:EUC-JP")) }
row("mode r:ext:int")     { |fd| encs(IO.new(fd, "r:EUC-JP:UTF-8")) }
# A `-` where the internal encoding goes asks for NO transcoding. There is no
# encoding by that name, so looking one up would warn about correct spelling.
row("mode r:ext:-")       { |fd| encs(IO.new(fd, "r:EUC-JP:-")) }
row("mode: keyword")      { |fd| encs(IO.new(fd, mode: "r:EUC-JP")) }
row("mode as O_ flags")   { |fd| encs(IO.new(fd, File::RDONLY)) }
# `b` claims the external slot only when nothing NAMED an encoding.
row("mode rb:enc")        { |fd| io = IO.new(fd, "rb:UTF-8"); [[io.external_encoding.name, io.binmode?], io] }

# --- the options Hash --------------------------------------------------------
row("external_encoding:")  { |fd| encs(IO.new(fd, external_encoding: "EUC-JP")) }
row("internal_encoding:")  { |fd| encs(IO.new(fd, external_encoding: "EUC-JP", internal_encoding: "UTF-8")) }
row("internal_encoding: -"){ |fd| encs(IO.new(fd, external_encoding: "EUC-JP", internal_encoding: "-")) }
row("internal_encoding: nil") { |fd| encs(IO.new(fd, external_encoding: "EUC-JP", internal_encoding: nil)) }
row("encoding:")           { |fd| encs(IO.new(fd, encoding: "EUC-JP")) }
row("encoding: ext:int")   { |fd| encs(IO.new(fd, encoding: "EUC-JP:UTF-8")) }
row("encoding: ext:-")     { |fd| encs(IO.new(fd, encoding: "EUC-JP:-")) }
row("binmode:")            { |fd| encs(IO.new(fd, binmode: true)) }
row("textmode:")           { |fd| encs(IO.new(fd, textmode: true)) }
row("path:")               { |fd| io = IO.new(fd, path: "labelled"); [io.path, io] }
row("autoclose: false")    { |fd| io = IO.new(fd, autoclose: false); [io.autoclose?, io] }
# An option nobody knows is ignored, not refused.
row("unknown option")      { |fd| io = IO.new(fd, no_such_option: 1); [io.class, io] }

# --- mode AND options together: the shape irb needs --------------------------
row("mode + opts")         { |fd| encs(IO.new(fd, "r", external_encoding: "EUC-JP", internal_encoding: "-")) }
row("open + mode + opts")  { |fd| encs(IO.open(fd, "r", external_encoding: "EUC-JP", internal_encoding: "-")) }
row("for_fd + mode + opts"){ |fd| encs(IO.for_fd(fd, "r", external_encoding: "EUC-JP", internal_encoding: "-")) }

# --- what is refused ---------------------------------------------------------
# An access mode naming nothing is refused at the call, not on the first read.
row("bad access mode")     { |fd| encs(IO.new(fd, "zz")) }
# The read/write bits asked for must be a subset of the descriptor's own.
row("mode the fd cannot serve") { |fd| encs(IO.new(fd, "w")) }
row("O_RDWR over a read-only fd") { |fd| encs(IO.new(fd, File::RDWR)) }
row("encoding named twice"){ |fd| encs(IO.new(fd, "r:EUC-JP", encoding: "UTF-8")) }
row("mode is not a String"){ |fd| encs(IO.new(fd, Object.new)) }
begin
  IO.new("/etc/hosts")
rescue TypeError => e
  puts "fd is not an Integer\t#{e.message}"
end

# --- the block form still closes --------------------------------------------
fd = IO.sysopen(SRC, "r")
p IO.open(fd, "r:EUC-JP") { |io| [io.external_encoding.name, io.read(5)] }

# --- a write handle ----------------------------------------------------------
DST = File.join(DIR, "dst.txt")
File.binwrite(DST, "")
wfd = IO.sysopen(DST, "w")
w = IO.new(wfd, "w", external_encoding: "EUC-JP", internal_encoding: "-")
p [w.external_encoding.name, w.internal_encoding&.name]
w.write("written")
w.close
p File.read(DST)

FileUtils.remove_entry(DIR)
puts "ran to the end"
