# A Hash that may be nil, past its nil-guard, whose `[]` value is then passed
# straight into a method that expects a particular type. The value has to
# arrive as itself: the guard tells you the hash is there, and the read has
# to hand over a Symbol rather than whatever a mixed-value hash defaults to.
#
# The idiom is a router: `matched = Router.match(...)`, `return if
# matched.nil?`, then `matched[:controller]` and `matched[:action]` into two
# methods that each take a Symbol.

module Counter
  def self.double(n)
    n * 2
  end
end

class A
  def act(n)
    n + 100
  end
end

class B
  def act(n)
    n + 200
  end
end

def fetch(present)
  if present
    { value: 21, label: "x" }
  else
    nil
  end
end

def pick(flag)
  if flag
    A.new
  else
    B.new
  end
end

m = fetch(true)
if m.nil?
  puts "missing"
else
  # Module class method dispatch — exercises
  # compile_expr_for_expected_type via
  # compile_constant_recv_expr → compile_call_args_with_defaults.
  puts Counter.double(m[:value])
  # Polymorphic-receiver dispatch arm — exercises
  # compile_poly_method_call's arm-arg unbox.
  puts pick(true).act(m[:value])
  puts pick(false).act(m[:value])
end

# Same shape with str_poly_hash?.
def fetch_str(present)
  if present
    { "k1" => 7, "k2" => "v" }
  else
    nil
  end
end

s = fetch_str(true)
if s.nil?
  puts "missing"
else
  puts Counter.double(s["k1"])
end
__END__
42
121
221
14
