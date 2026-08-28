# Regexp.new's second argument, when it is a String, carries flag LETTERS
# rather than a truthy value: "x" is EXTENDED, not IGNORECASE. Any order and
# any repetition of m/i/x is read, the empty string is no options, and
# anything else is an ArgumentError naming the letter -- `n`, `u` and `o` are
# unknown to CRuby here too.
def show(o)
  r = Regexp.new("a", o)
  puts "#{o.inspect} => #{r.options} #{r.inspect}"
rescue ArgumentError => e
  puts "#{o.inspect} => ArgumentError: #{e.message}"
end

show("i")
show("m")
show("x")
show("mix")
show("xi")
show("ii")
show("")
show("z")
show("X")
show("o")
show("n")
show("u")

# and through the routes that leave the value poly
v = "m"
show(v)
["x"].each { |s| show(s) }
[["a", "im"]].each { |p, o| r = Regexp.new(p, o); puts "destructured => #{r.options} #{r.inspect}" }

# the letters reach the match, not just #options
p("A" =~ Regexp.new("a", "i"))
p("A" =~ Regexp.new("a", ""))
p("ab" =~ Regexp.new("a b", "x"))
p("a\nb" =~ Regexp.new("a.b", "m"))
