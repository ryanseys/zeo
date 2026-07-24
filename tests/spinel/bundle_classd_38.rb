# Bundled tests:
#   - send
#   - send_double_underscore

# === send ===
# Test send(:literal_symbol)
# Note: send rewrite requires Ruby parser (spinel_parse.rb)
x = [1,2,3]
puts x.length

class T_send_Adder
  def initialize(n)
    @n = n
  end
  def add(x)
    @n + x
  end
end
a = T_send_Adder.new(10)
puts a.add(5)

# === send_double_underscore ===
# .__send__(:sym, args) and .__send__("sym", args) statically rewrite
# to .sym(args) at parse time, identical to .send. CRuby exposes
# __send__ as the overrides-resistant alias of send; Spinel does not
# model the visibility distinction, so the two are semantically
# equivalent here.

class T_send_double_underscore_Calc
  def add(a, b)
    a + b
  end

  def hello
    "hi"
  end
end

c = T_send_double_underscore_Calc.new

# Symbol arm: .__send__(:sym, args) -> .sym(args).
puts c.__send__(:add, 10, 20)   # 30
puts c.__send__(:hello)         # hi

# String arm: .__send__("sym", args) -> .sym(args).
puts c.__send__("add", 3, 4)    # 7
puts c.__send__("hello")        # hi

puts "done"

