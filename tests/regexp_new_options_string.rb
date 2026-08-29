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

v = "m"
show(v)
["x"].each { |s| show(s) }
[["a", "im"]].each { |p, o| r = Regexp.new(p, o); puts "destructured => #{r.options} #{r.inspect}" }

p("A" =~ Regexp.new("a", "i"))
p("A" =~ Regexp.new("a", ""))
p("ab" =~ Regexp.new("a b", "x"))
p("a\nb" =~ Regexp.new("a.b", "m"))
