# Three intertwined fixes around kwarg param inference on an
# inherited instance method:
#
# 1. Analyze: the inh_ branch (bare call inside a class body that
#    resolves to a parent's method) was iterating arg_ids by
#    position, so a trailing `KeywordHashNode` landed on whatever
#    positional slot came next instead of the matching kwarg
#    slot. `head(204, content_type: "x")` widened `content_type`'s
#    slot with the *whole keyword hash type* (sym_str_hash) and
#    left it un-pinned to "string". Route through
#    widen_ptypes_from_args so each `key: value` lands on the
#    named-param slot.
#
# 2. Codegen: compile_typed_call_args had the same positional-
#    only loop on the emit side. Extract KeywordHashNode pairs
#    up front, inject them into arg_ids at the matching named
#    slot, and route the existing per-arg type-coerce loop
#    through that. Without this, the kwarg expression
#    disappeared at the call site — `head(204, content_type:
#    "application/json")` emitted `sp_Base_head(self, 204, 0)`.
#
# 3. Analyze ivar heterogeneity: the same-base nullable variant
#    case (`@x = "hi"` + `@x = some_nullable_string_param`)
#    counted as 2 distinct observations and widened the ivar to
#    poly. Both write the same C storage (const char *) with
#    NULL as the nil sentinel. update_ivar_type and
#    finalize_ivar_heterogeneity now treat T + T? as a single
#    nullable observation, preserving the typed pointer slot.

class Base
  def initialize
    @content_type = "text/html"
  end

  def head(status, content_type: nil)
    @status = status
    @content_type = content_type unless content_type.nil?
    nil
  end

  def show_content_type
    @content_type
  end
end

class Child < Base
  def render_json
    head(204, content_type: "application/json")
  end

  def render_html
    head(200)
  end
end

c = Child.new
c.render_json
puts c.show_content_type
c2 = Child.new
c2.render_html
puts c2.show_content_type
