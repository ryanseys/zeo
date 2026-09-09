# instance_variable_get and formatting answer a copy, so mutating the answer
# does not reach the object; instance_variable_set does share.
# (spinel issue #3227)
class W
  attr_reader :a, :b
  def initialize
    @a = +"aa"
    @b = "bb"
    t = @a
    @a << "!"
    p t
  end
end
w = W.new
p w.a
p w.instance_variable_get(:@a)
puts "#{w.a}/#{w.b}"
p({ x: w.a })
p [w.a, w.b]
p w.a.length
p w.a == "aa!"
s = w.a.dup
s << "?"
p s
p w.a
w.instance_variable_set(:@a, "set")
p w.a
__END__
"aa!"
"aa!"
"aa!"
aa!/bb
{x: "aa!"}
["aa!", "bb"]
3
true
"aa!?"
"aa!"
"set"
