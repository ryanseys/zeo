class Pat
  def initialize(s); @s = s; end
  def =~(str); str.include?(@s) ? 0 : nil; end
end
p Pat.new("x") !~ "xyz"
p Pat.new("q") !~ "xyz"
p("abc" !~ /b/)
p("abc" !~ /z/)
r = /d/
p("abc" !~ r)
s = "abc"
p(s !~ /a/)
