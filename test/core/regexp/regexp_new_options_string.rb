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
__END__
"i" => 1 /a/i
"m" => 4 /a/m
"x" => 2 /a/x
"mix" => 7 /a/mix
"xi" => 3 /a/ix
"ii" => 1 /a/i
"" => 0 /a/
"z" => ArgumentError: unknown regexp option: z
"X" => ArgumentError: unknown regexp option: X
"o" => ArgumentError: unknown regexp option: o
"n" => ArgumentError: unknown regexp option: n
"u" => ArgumentError: unknown regexp option: u
"m" => 4 /a/m
"x" => 2 /a/x
destructured => 5 /a/mi
0
nil
0
0
