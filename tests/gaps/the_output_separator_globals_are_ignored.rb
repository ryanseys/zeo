# `$,` (the output field separator) and `$\` (the output record separator)
# are settable and readable in zeo, and nothing reads them: `print` and
# `Array#join` behave as though both were nil.
#
#   $, = "-"; io.print("a", "b")   ruby "a-b"   zeo "ab"
#   $\ = "!"; io.print("a")        ruby "a!"    zeo "a"
#   $, = "-"; [1, 2].join          ruby "1-2"   zeo "12"
#
# The globals themselves work -- reading one back gives what was written,
# and their defaults are nil -- so this is not a missing variable but a
# missing READER at the three sites that consult them. `IO#write` correctly
# ignores `$,` (ruby does too: only `print` joins its arguments with it), and
# `puts` correctly ignores both, which is what makes the three sites exact
# rather than a family.
#
# Worth stating because it argues the fix is small: ruby resolves both at the
# CALL, not at stream creation, so a program that sets `$,` around one
# `print` and clears it after gets the separator for that call only.
#
# Oracle: both separators are honoured.
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
