# A Regexp is keyed by its SOURCE and flags, which is ruby's own
# `Regexp#eql?`/`#hash` rule: `/a/` written twice is one Hash key, `/a/` and
# `/a/i` are two. The key projection used to fall through to the object
# pointer, so a Regexp key never matched and `#hash` disagreed between two
# equal literals.

h = { /re/ => "y", /re/i => "i", /re/m => "m", /other/ => "o" }
p h[/re/], h[/re/i], h[/re/m], h[/other/], h[/nope/]
p h.size
p(/a/.hash == /a/.hash)
p(/a/.hash == /a/i.hash)
p(/a/.eql?(/a/))
p Regexp.new("a").hash == /a/.hash
p({ /x/ => 1 } == { /x/ => 1 })
p [/a/, /a/, /b/].uniq
p({ /a/ => 1 }.key?(/a/))
__END__
"y"
"i"
"m"
"o"
nil
4
true
false
true
true
true
[/a/, /b/]
true
