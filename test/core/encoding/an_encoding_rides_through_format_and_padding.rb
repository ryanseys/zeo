# The encoding a derived String carries, across the four places that build
# one out of two others: interpolation, `%`/`format`, `Array#pack` and the
# `ljust`/`rjust`/`center` family.
#
# The rule is one rule -- CRuby's `rb_enc_check`. An ASCII-only side never
# moves the answer, because every encoding zeo carries is ASCII-compatible;
# two non-ASCII sides in different encodings do not combine at all, and that
# is an `Encoding::CompatibilityError`.
#
# zeo used to tag every one of these UTF-8, which lost the receiver's
# encoding, lost the argument's, and never raised.

def show(label)
  v = yield
  puts format("%-34s %s", label, v.is_a?(String) ? "#{v.encoding} #{v.bytes.inspect}" : v.inspect)
rescue => e
  puts format("%-34s %s", label, e.class)
end

utf8   = "café"
binary = utf8.b
ascii  = "abc"
latin1 = "\xe9".force_encoding("ISO-8859-1")

# -- interpolation ---------------------------------------------------------
show("interp ascii + latin1")   { "#{ascii}#{latin1}" }
show("interp latin1 + ascii")   { "#{latin1}#{ascii}" }
show("interp utf8 + latin1")    { "#{utf8}#{latin1}" }
show("interp ascii only")       { "#{ascii}#{ascii}" }
show("interp binary")           { "#{ascii}#{binary}" }

# -- format ----------------------------------------------------------------
show("%s ascii fmt, binary arg") { "%s" % binary }
show("%s latin1 fmt, ascii arg") { "%s".force_encoding("ISO-8859-1") % ascii }
show("%d binary fmt")            { "%d".b % 5 }
show("%s no substitution")       { "plain".b % [] }
show("%s two args, one binary")  { "%s%s" % [ascii, binary] }
show("%s incompatible pair")     { "%s%s" % [utf8, latin1] }
show("format() keeps it")        { format("%s", binary) }

# -- pack ------------------------------------------------------------------
# A template starts at US-ASCII and only ever moves down: `m`/`M`/`u` write
# ASCII text, `U` lifts to UTF-8, and anything writing raw bytes pins the
# answer at ASCII-8BIT.
show("pack empty")               { [].pack("") }
show("pack m")                   { ["hi"].pack("m0") }
show("pack U")                   { [233].pack("U*") }
show("pack U then m")            { [65, "hi"].pack("Um") }
show("pack U then C")            { [65, 66].pack("UC") }
show("pack C")                   { [65].pack("C*") }

# -- padding ---------------------------------------------------------------
show("ljust latin1 pad")         { ascii.ljust(5, latin1) }
show("rjust latin1 pad")         { ascii.rjust(5, latin1) }
show("center latin1 pad")        { ascii.center(5, latin1) }
show("ljust already wide")       { ascii.ljust(1, latin1) }
show("ljust incompatible")       { utf8.ljust(9, latin1) }
show("ljust incompatible, wide") { utf8.ljust(1, latin1) }

# -- the comparison the same rule decides ----------------------------------
show("binary <=> utf8")          { binary <=> utf8 }
show("utf8 <=> binary")          { utf8 <=> binary }
show("ascii binary <=> ascii")   { ascii.b <=> ascii }
p [utf8, binary, ascii].sort.map { _1.encoding.to_s }

# -- transcoding out of BINARY --------------------------------------------
# Binary has no invalid byte, so a high byte is a valid character with no
# mapping: `:undef` territory, not `:invalid`.
show("binary high byte to utf8") { binary.encode("UTF-8") }
puts(begin
  binary.encode("UTF-8")
rescue => e
  "#{e.class}: #{e.message}"
end)
__END__
interp ascii + latin1              ISO-8859-1 [97, 98, 99, 233]
interp latin1 + ascii              ISO-8859-1 [233, 97, 98, 99]
interp utf8 + latin1               Encoding::CompatibilityError
interp ascii only                  UTF-8 [97, 98, 99, 97, 98, 99]
interp binary                      ASCII-8BIT [97, 98, 99, 99, 97, 102, 195, 169]
%s ascii fmt, binary arg           ASCII-8BIT [99, 97, 102, 195, 169]
%s latin1 fmt, ascii arg           ISO-8859-1 [97, 98, 99]
%d binary fmt                      ASCII-8BIT [53]
%s no substitution                 ASCII-8BIT [112, 108, 97, 105, 110]
%s two args, one binary            ASCII-8BIT [97, 98, 99, 99, 97, 102, 195, 169]
%s incompatible pair               Encoding::CompatibilityError
format() keeps it                  ASCII-8BIT [99, 97, 102, 195, 169]
pack empty                         US-ASCII []
pack m                             US-ASCII [97, 71, 107, 61]
pack U                             UTF-8 [195, 169]
pack U then m                      UTF-8 [65, 97, 71, 107, 61, 10]
pack U then C                      ASCII-8BIT [65, 66]
pack C                             ASCII-8BIT [65]
ljust latin1 pad                   ISO-8859-1 [97, 98, 99, 233, 233]
rjust latin1 pad                   ISO-8859-1 [233, 233, 97, 98, 99]
center latin1 pad                  ISO-8859-1 [233, 97, 98, 99, 233]
ljust already wide                 UTF-8 [97, 98, 99]
ljust incompatible                 Encoding::CompatibilityError
ljust incompatible, wide           Encoding::CompatibilityError
binary <=> utf8                    -1
utf8 <=> binary                    1
ascii binary <=> ascii             0
["UTF-8", "ASCII-8BIT", "UTF-8"]
binary high byte to utf8           Encoding::UndefinedConversionError
Encoding::UndefinedConversionError: "\xC3" from ASCII-8BIT to UTF-8
