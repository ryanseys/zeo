# Ruby warns at PARSE time when a literal hash key is written twice: the
# second value wins silently, so the warning is the only sign. It names the
# line the surviving value is on, which is not always the line of the key it
# overwrote.
#
# These are the only warnings zeo forwards. They arrive on stderr before any
# program output, which is where CRuby prints them too -- see the sidecar
# golden.

# --- a hash literal ----------------------------------------------------------
p({ k: 1, k: 2 })
p({ "s" => 1, "s" => 2 })
p({ 1 => :a, 1 => :b })

# --- across lines: the warning points at the WINNER ---------------------------
p({
  dup: 1,
  other: 2,
  dup: 3,
})

# --- keyword arguments are the same list ------------------------------------
def capture(**opts) = opts
p capture(a: 1, a: 2)

# --- a splat colliding with a literal pair ----------------------------------
p({ x: 0, **{ x: 9 } })
p capture(**{ y: 1 }, y: 2)

# --- inside a method body, and inside a block -------------------------------
def build = { m: 1, m: 2 }
p build
p [1].map { { b: 1, b: 2 } }

# --- three of the same key warn twice ---------------------------------------
p({ t: 1, t: 2, t: 3 })

# --- keys that only LOOK duplicated do not warn ------------------------------
p({ a: 1, b: 2 })
p({ "a" => 1, a: 2 })
p({ 1 => :int, "1" => :str, :"1" => :sym })
n = 1
p({ n => :first, 2 => :second })
__END__
{k: 2}
{"s" => 2}
{1 => :b}
{dup: 3, other: 2}
{a: 2}
{x: 9}
{y: 2}
{m: 2}
[{b: 2}]
{t: 3}
{a: 1, b: 2}
{"a" => 1, a: 2}
{1 => :int, "1" => :str, "1": :sym}
{1 => :first, 2 => :second}
#@ stderr
core/object/parse_warning_duplicate_key.rb:11: warning: key :k is duplicated and overwritten on line 11
core/object/parse_warning_duplicate_key.rb:12: warning: key "s" is duplicated and overwritten on line 12
core/object/parse_warning_duplicate_key.rb:13: warning: key 1 is duplicated and overwritten on line 13
core/object/parse_warning_duplicate_key.rb:17: warning: key :dup is duplicated and overwritten on line 19
core/object/parse_warning_duplicate_key.rb:24: warning: key :a is duplicated and overwritten on line 24
core/object/parse_warning_duplicate_key.rb:27: warning: key :x is duplicated and overwritten on line 27
core/object/parse_warning_duplicate_key.rb:28: warning: key :y is duplicated and overwritten on line 28
core/object/parse_warning_duplicate_key.rb:31: warning: key :m is duplicated and overwritten on line 31
core/object/parse_warning_duplicate_key.rb:33: warning: key :b is duplicated and overwritten on line 33
core/object/parse_warning_duplicate_key.rb:36: warning: key :t is duplicated and overwritten on line 36
core/object/parse_warning_duplicate_key.rb:36: warning: key :t is duplicated and overwritten on line 36
