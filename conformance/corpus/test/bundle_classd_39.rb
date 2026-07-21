# Bundled tests:
#   - send_literal_string
#   - send_literal_symbol

# === send_literal_string ===
# .send("sym", args) plain string literal rewrites to .sym(args) at
# parse time, identical to the :symbol form. Interpolated strings and
# variable args still fall through to runtime dispatch.

class T_send_literal_string_Calc
  def add(a, b)
    a + b
  end

  def hello
    "hi"
  end
end

c = T_send_literal_string_Calc.new
puts c.send("add", 2, 3)       # 5
puts c.send("hello")           # hi

puts "done"

# === send_literal_symbol ===
# .send(:sym, args) statically rewrites to .sym(args) at parse time.
# Non-literal symbol args (variable, interpolation) leave the call
# alone -- those require runtime dispatch which Spinel doesn't model.

class T_send_literal_symbol_Calc
  def add(a, b)
    a + b
  end

  def double(x)
    x * 2
  end

  def hello
    "hi"
  end

  # Returns the input length; used to exercise quoted-paren args.
  def length_of(s)
    s.length
  end
end

# 1. Basic shapes.
c = T_send_literal_symbol_Calc.new
puts c.send(:add, 3, 4)        # 7
puts c.send(:double, 21)       # 42
puts c.send(:hello)            # hi

# 2. Quoted parens inside args must not prematurely close the call.
#    The rewriter tracks `"..."` state so `)` inside the string is
#    treated as a literal character, not as the call's close paren.
puts c.send(:length_of, "a)b")     # 3
puts c.send(:length_of, "x(y)z")   # 5
puts c.send(:length_of, 'p)q')     # 3

# 3. Operator method `[]` via send rewrites to .[](idx) which Prism
#    parses as the index operator.
arr = [10, 20, 30]
puts arr.send(:[], 0)          # 10
puts arr.send(:[], 2)          # 30

# 4. Operator method `[]=` via send rewrites to .[]=(idx, val).
arr.send(:[]=, 1, 99)
puts arr[0]                    # 10
puts arr[1]                    # 99
puts arr[2]                    # 30

puts "done"

