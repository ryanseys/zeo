# #493. `raise ... unless x.is_a?(C)` (and the equivalent
# `if !x.is_a?(C); raise ...; end` shape) is a definite-throw
# type assertion: when control reaches the statement after the
# guard, `x` is provably of class `C`. The repro from the issue:
#
#   def process(x)
#     raise TypeError, "expected A" unless x.is_a?(A)
#     puts x.body
#   end
#
# pre-fix emitted a full cls_id-switch over every class with a
# same-named field after the guard (sp_box_int for B#body's int,
# sp_box_str for A#body's string, then sp_poly_puts on the boxed
# result). post-fix the guard's narrow flows through to a single
# cls_id arm and the field read lowers to a direct
# `((sp_A *)lv_x.v.p)->iv_body` chain.
#
# Implementation: the analyze-side body walkers
# (scan_new_calls / collect_return_types_nid /
# scan_cls_method_calls) plus the codegen-side compile_body_return
# now push a sibling-scope type narrow when a stmt's
# parse_raise_guard_narrow recognizes the shape; the narrow stays
# pushed for the remainder of the StatementsNode. infer_type at
# codegen consults the narrow stack before honouring the
# pre-narrow @nd_inferred_type cache for `LV(narrowed).attr_reader`
# call shapes, so attr_reader resolution returns the narrowed
# class's static ivar type rather than the wider pre-narrow poly.

class A
  attr_accessor :body
  def initialize; @body = "from-A"; end
end

class B
  attr_accessor :body
  def initialize; @body = 42; end
end

def pick(n)
  n > 0 ? A.new : B.new
end

def process_unless(x)
  raise "expected A" unless x.is_a?(A)
  puts x.body
end

def process_if_negated(x)
  if !x.is_a?(A)
    raise "expected A"
  end
  puts x.body
end

# Both spellings should resolve identically -- single A#body
# dispatch, no boxing through sp_RbVal.
process_unless(pick(1))
process_if_negated(pick(1))

# Multiple guards in sequence accumulate narrows.
class S
  attr_accessor :name
  def initialize; @name = "sized"; end
end

def render(s, t)
  raise "expected S" unless s.is_a?(S)
  raise "expected S" unless t.is_a?(S)
  puts s.name + "/" + t.name
end

render(S.new, S.new)
