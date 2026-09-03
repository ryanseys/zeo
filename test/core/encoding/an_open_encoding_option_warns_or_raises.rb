# The two rules an open-time encoding argument follows, which are not the same
# rule and are not derivable from each other.
#
#   * A SPEC -- a mode string's `:ext[:int]` tail, or a String `encoding:` --
#     goes through CRuby's `parse_mode_enc`. A name it does not know WARNS and
#     leaves the slot at its default.
#   * `external_encoding:` / `internal_encoding:` go through `rb_to_encoding`.
#     A name they do not know RAISES.
#
# And a spec's internal half spelled `-` asks for NO transcoding rather than
# naming an encoding: `Encoding.find("-")` raises, so looking it up would warn
# about a spelling that is correct. zeo raised where ruby warns, and warned
# about `-`, so `File.open(f, "r:UTF-8:-")` -- the shape irb reads its history
# file with -- printed a warning ruby does not.
require "tmpdir"
require "fileutils"

DIR = Dir.mktmpdir("zeo_open_enc")
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

# --- a spec warns ------------------------------------------------------------
row("mode tail unknown")     { File.open(SRC, "r:NoSuchEnc") { |f| encs(f) } }
row("encoding: unknown")     { File.open(SRC, "r", encoding: "NoSuchEnc") { |f| encs(f) } }
row("encoding: pair, internal unknown") { File.open(SRC, "r", encoding: "UTF-8:NoSuchEnc") { |f| encs(f) } }
row("read encoding: unknown"){ File.read(SRC, encoding: "NoSuchEnc") }

# --- an encoding ARGUMENT raises --------------------------------------------
row("external_encoding: unknown")  { File.open(SRC, "r", external_encoding: "NoSuchEnc") { |f| encs(f) } }
row("internal_encoding: unknown")  { File.open(SRC, "r", internal_encoding: "NoSuchEnc") { |f| encs(f) } }
row("read external_encoding: unknown") { File.read(SRC, external_encoding: "NoSuchEnc") }
# `-` is not a name, so it is not one here either.
row("external_encoding: -")        { File.open(SRC, "r", external_encoding: "-") { |f| encs(f) } }
row("Encoding.find(-)")            { Encoding.find("-") }

# --- `-` means no transcoding -----------------------------------------------
row("mode tail ext:-")       { File.open(SRC, "r:EUC-JP:-") { |f| encs(f) } }
row("mode tail utf8:-")      { File.open(SRC, "r:UTF-8:-") { |f| encs(f) } }
row("mode tail utf8:- reads"){ File.open(SRC, "r:UTF-8:-", &:read) }
row("internal_encoding: -")  { File.open(SRC, "r", external_encoding: "EUC-JP", internal_encoding: "-") { |f| encs(f) } }
row("encoding: ext:-")       { File.open(SRC, "r", encoding: "EUC-JP:-") { |f| encs(f) } }
row("read mode: ext:-")      { File.read(SRC, mode: "r:UTF-8:-") }
row("readlines mode: ext:-") { File.readlines(SRC, mode: "r:UTF-8:-") }
row("read internal: -")      { File.read(SRC, external_encoding: "EUC-JP", internal_encoding: "-").encoding.name }

# --- naming an encoding in both places is refused ---------------------------
row("mode tail and encoding:")          { File.open(SRC, "r:EUC-JP", encoding: "UTF-8") { |f| encs(f) } }
row("mode tail and external_encoding:") { File.open(SRC, "r:EUC-JP", external_encoding: "UTF-8") { |f| encs(f) } }

# --- perm and options are BOTH optional and both trail the mode -------------
DST = File.join(DIR, "dst.txt")
row("open with perm and opts") { File.open(DST, "w", 0o600, external_encoding: "EUC-JP") { |f| encs(f) } }
row("perm still applied")      { format("%o", File.stat(DST).mode & 0o777) }
row("open with opts, no perm") { File.open(DST, "w", external_encoding: "EUC-JP") { |f| encs(f) } }

FileUtils.remove_entry(DIR)
puts "ran to the end"
__END__
mode tail unknown	["UTF-8", nil]
encoding: unknown	["UTF-8", nil]
encoding: pair, internal unknown	["UTF-8", nil]
read encoding: unknown	"hello\n"
external_encoding: unknown	ArgumentError: unknown encoding name - NoSuchEnc
internal_encoding: unknown	ArgumentError: unknown encoding name - NoSuchEnc
read external_encoding: unknown	ArgumentError: unknown encoding name - NoSuchEnc
external_encoding: -	ArgumentError: unknown encoding name - -
Encoding.find(-)	ArgumentError: unknown encoding name - -
mode tail ext:-	["EUC-JP", nil]
mode tail utf8:-	["UTF-8", nil]
mode tail utf8:- reads	"hello\n"
internal_encoding: -	["EUC-JP", nil]
encoding: ext:-	["EUC-JP", nil]
read mode: ext:-	"hello\n"
readlines mode: ext:-	["hello\n"]
read internal: -	"EUC-JP"
mode tail and encoding:	ArgumentError: encoding specified twice
mode tail and external_encoding:	ArgumentError: encoding specified twice
open with perm and opts	["EUC-JP", nil]
perm still applied	"600"
open with opts, no perm	["EUC-JP", nil]
ran to the end
#@ stderr
core/encoding/an_open_encoding_option_warns_or_raises.rb:33: warning: Unsupported encoding NoSuchEnc ignored
core/encoding/an_open_encoding_option_warns_or_raises.rb:34: warning: Unsupported encoding NoSuchEnc ignored
core/encoding/an_open_encoding_option_warns_or_raises.rb:35: warning: Unsupported encoding NoSuchEnc ignored
core/encoding/an_open_encoding_option_warns_or_raises.rb:36: warning: Unsupported encoding NoSuchEnc ignored
