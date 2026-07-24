# When a `sym_poly_hash?` (or `str_poly_hash?`) survives a nil-
# guard and its `[]` value flows into a typed-param call site,
# analyze's cached infer for `recv[k]` falls through to int
# (the leaf-type dispatch rejects the `?`-suffixed recv name), so
# downstream the call site sees `at == "int"` and skips the
# poly→int unbox. The actual C emit is sp_<*PolyHash>_get(...)
# returning sp_RbVal — the call then passes sp_RbVal into a
# mrb_int slot.
#
# Codegen-side fix: expr_emits_poly_rb_val recognizes the shape
# at emit time (independent of analyze cache) and the unbox
# fires in compile_expr_for_expected_type and the poly-dispatch
# arm.
#
# Real repro lives in roundhouse spinel-blog main.rb:
#   matched = Router.match(...)
#   return if matched.nil?           # matched still : sym_poly_hash?
#   controller = Main.instantiate_controller(matched[:controller])
#   controller.process_action(matched[:action])
# Both `instantiate_controller` (Symbol param) and the poly
# dispatch of `process_action` (Symbol param) caught the bug.

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
