# A non-interpolated regexp literal is ONE frozen object per site, cached in
# the runtime under an emitter-assigned id. A program's ids are dense from
# zero and minted by one compile; a run-time `eval` is compiled by a FRESH
# compiler that starts at zero again, so the two spaces collided and a site
# answered whichever pattern reached the cache first.
#
# It is silent: both sides get a working Regexp, just the wrong one. The
# tools found it -- the env-var check scanned every doc with a pattern
# from a run-time-required unit and reported the whole file as an undefined
# variable name.
#
# `flipflop::EVAL_BASE` and the `using` slots already hold their id spaces
# apart for exactly this reason; regexp literals and `attach_function` symbol
# sites did not.

text = "a ZEO_ONE b ZEO_TWO c"
names = /\bZEO_[A-Z0-9_]+\b/

# The snippet's own literals come first, so its site 0 is minted before the
# program reaches its own.
eval(%q{
  def words(s) = s.scan(/\w+/)
  def digits(s) = s.scan(/\d+/)
})

p text.scan(names)
p words("hi there")
p digits("a1 b22 c333")
p text.scan(names)
p text.scan(/\bZEO_[A-Z0-9_]+\b/)

# A second snippet must not collide with the first either.
eval(%q{ def caps(s) = s.scan(/[A-Z]+/) })
p caps("aBcDe")
p words("still here")

# The identity a per-site cache exists for: one site, one frozen object.
two = [text.scan(names), text.scan(names)]
p two.first == two.last
p names.frozen?
__END__
["ZEO_ONE", "ZEO_TWO"]
["hi", "there"]
["1", "22", "333"]
["ZEO_ONE", "ZEO_TWO"]
["ZEO_ONE", "ZEO_TWO"]
["B", "D"]
["still", "here"]
true
true
