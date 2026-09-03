# `$,` (the output field separator) and `$\` (the output record separator)
# are honoured by the three sites that read them: `print` on any stream and
# `Array#join`.
#
# Ruby resolves both at the CALL, not at stream creation, so a program that
# sets `$,` around one `print` and clears it after gets the separator for
# that call only -- which is why every reader asks
# `kernel::output_separator` rather than caching one.
#
# The two sites that DON'T read them are the point of the last two cases:
# `IO#write` ignores `$,` (only `print` joins its arguments with it), and
# `puts` ignores both.

require "stringio"
s = StringIO.new("".dup)
$, = "-"
s.print("a", "b")
$, = nil
p s.string

t = StringIO.new("".dup)
$\ = "!"
t.print("a")
$\ = nil
p t.string

$, = "-"
p [1, 2].join
$, = nil

u = StringIO.new("".dup)
$, = "-"
u.write("a", "b")
$, = nil
p u.string

v = StringIO.new("".dup)
$\ = "!"
v.puts("a")
$\ = nil
p v.string
__END__
"a-b"
"a!"
"1-2"
"ab"
"a\n"
