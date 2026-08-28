# Regexp.new's second argument is an Integer of option bits, or nil/false for
# none, or anything else truthy for IGNORECASE -- a choice CRuby makes from the
# VALUE. Where the analyzer could not see the type statically the choice was
# settled on the truthy arm, so an option that arrived as a block parameter, a
# hash read or a method return answered /i whatever it held, 0 included.
def show(label, re)
  puts "#{label}: #{re.options} #{re.inspect}"
end

show("literal", Regexp.new("a", 4))
f = 2
show("local", Regexp.new("a", f))
[6].each { |x| show("block param", Regexp.new("a", x)) }
[["a", 3]].each { |s, g| show("destructured", Regexp.new(s, g)) }
h = { "k" => 5 }
show("hash read", Regexp.new("a", h["k"]))
show("hash miss", Regexp.new("a", h["nope"]))
def opts; 1; end
show("method ret", Regexp.new("a", opts))

# zero must not read as truthy, by any route
show("zero literal", Regexp.new("a", 0))
[0].each { |z| show("zero via block", Regexp.new("a", z)) }
[["a", 0]].each { |s, z| show("zero destructured", Regexp.new(s, z)) }

# the arms CRuby keeps for a non-Integer
show("nil", Regexp.new("a", nil))
show("false", Regexp.new("a", false))
show("true", Regexp.new("a", true))

# and the flags reach the match, not just #options
[["A", 1]].each { |s, g| p("a" =~ Regexp.new(s, g)) }
[["A", 0]].each { |s, g| p("a" =~ Regexp.new(s, g)) }
[["a b", 2]].each { |s, g| p("ab" =~ Regexp.new(s, g)) }
[["a.b", 4]].each { |s, g| p("a\nb" =~ Regexp.new(s, g)) }
