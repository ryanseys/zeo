# A Hash written with BRACES is a positional argument; only keywords are
# options.
#
# `IO.new(fd, {external_encoding: "EUC-JP"})` hands the MODE slot a Hash, and
# CRuby answers `TypeError: no implicit conversion of Hash into String` --
# `rb_scan_args_kw` is told to use the caller's own semantics, so the mark the
# caller left on the Hash is what separates the two calls. `File.open` reads its
# mode the same way.
#
# Zeo peeled ANY trailing Hash off a `**kwrest` row, which could not tell them
# apart. The strict peel (`**opts!`) takes only a Hash the caller marked,
# leaving a braced one where the caller put it -- in the mode slot, where
# converting it to a String is the whole of CRuby's answer.
require "tmpdir"
require "fileutils"

DIR = Dir.mktmpdir("zeo_braced_mode")
SRC = File.join(DIR, "src.txt")
File.binwrite(SRC, "hello\n")

def row(name)
  puts "#{name}\t#{yield.inspect}"
rescue StandardError => e
  puts "#{name}\t#{e.class}: #{e.message}"
end

def encs(io)
  [io.external_encoding&.name, io.internal_encoding&.name]
end

OPTS = { external_encoding: "EUC-JP" }.freeze

# --- braced: a positional, so it is a mode ----------------------------------
row("IO.new braced")        { IO.new(IO.sysopen(SRC), { external_encoding: "EUC-JP" }) }
row("IO.new braced var")    { IO.new(IO.sysopen(SRC), OPTS) }
row("File.open braced")     { File.open(SRC, { external_encoding: "EUC-JP" }) }
row("File.open braced mode"){ File.open(SRC, { mode: "r" }) }

# --- keywords: the options ---------------------------------------------------
row("IO.new keywords") do
  io = IO.new(IO.sysopen(SRC), external_encoding: "EUC-JP")
  r = encs(io)
  io.close
  r
end
row("IO.new double-splat") do
  io = IO.new(IO.sysopen(SRC), **OPTS)
  r = encs(io)
  io.close
  r
end
row("File.open keywords")   { File.open(SRC, external_encoding: "EUC-JP") { |f| encs(f) } }
row("File.open double-splat") { File.open(SRC, **OPTS) { |f| encs(f) } }
# A hashrocket pair with no braces is still keywords, which is the spelling
# irb's `IO.open(fd, :external_encoding => enc)` uses.
row("File.open hashrocket") { File.open(SRC, :external_encoding => "EUC-JP") { |f| encs(f) } }
# ...and a mode BESIDE the keywords keeps its own slot.
row("mode then keywords")   { File.open(SRC, "r", external_encoding: "EUC-JP") { |f| encs(f) } }

FileUtils.remove_entry(DIR)
puts "ran to the end"
__END__
IO.new braced	TypeError: no implicit conversion of Hash into String
IO.new braced var	TypeError: no implicit conversion of Hash into String
File.open braced	TypeError: no implicit conversion of Hash into String
File.open braced mode	TypeError: no implicit conversion of Hash into String
IO.new keywords	["EUC-JP", nil]
IO.new double-splat	["EUC-JP", nil]
File.open keywords	["EUC-JP", nil]
File.open double-splat	["EUC-JP", nil]
File.open hashrocket	["EUC-JP", nil]
mode then keywords	["EUC-JP", nil]
ran to the end
