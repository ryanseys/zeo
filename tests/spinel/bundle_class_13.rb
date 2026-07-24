# Bundled tests:
#   - imeth_override_widen_to_poly
#   - implicit_self_param_poly

# === imeth_override_widen_to_poly ===
# #567 (Sam Ruby). Sibling of #563. When an override widens a
# value param to sp_RbVal (poly) because its call sites pass
# mixed types, but the parent's same param stays scalar, the C
# function signatures of base and override disagree. The
# override-dispatch gate cls_imeth_override_ptypes_match
# rejected the family, so a `self[k] = v` inside a parent body
# fell through to the static call on the parent's raise stub
# rather than dispatching to the override.
#
# Fix: post-fixpoint, unify the family's ptypes at any slot
# where at least one member is "poly" so all member signatures
# agree. The cls_id switch then fires and routes to the
# concrete override.

class T_imeth_override_widen_to_poly_Base
  def []=(name, value); raise NotImplementedError; end
  def fill; self[:title] = "Hello"; end
end

class T_imeth_override_widen_to_poly_Article < T_imeth_override_widen_to_poly_Base
  attr_accessor :id, :title
  def []=(name, value)
    case name
    when :id then @id = value
    when :title then @title = value
    end
  end
end

a = T_imeth_override_widen_to_poly_Article.new
a.id = 0
a.title = ""
a[:id] = 42
a[:title] = "Other"
a.fill
puts a.title.inspect

# === implicit_self_param_poly ===
class T_implicit_self_param_poly_TokenSink
  def initialize
    @value = ""
  end

  def set_token(value)
    @value = value.to_s
  end

  def run
    set_token(1)
    set_token("done")
    puts @value
  end
end

T_implicit_self_param_poly_TokenSink.new.run

