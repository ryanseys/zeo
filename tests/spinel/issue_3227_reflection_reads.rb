# Promoted-ivar reads through reflection and formatting paths hand out safe
# copies (never the raw handle); instance_variable_set joins the shared set.
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
