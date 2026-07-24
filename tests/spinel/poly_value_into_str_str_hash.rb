# Issue #651: an ivar inferred as poly (sp_RbVal) because a
# `kwarg: nil` default makes the writer's param nullable, then
# read at a `str_str_hash[key] = ivar` site, used to flow the
# boxed value into `sp_StrStrHash_set(_, _, sp_RbVal)` where the
# C signature expects `const char *`. C compile failed with
# "passing sp_RbVal to parameter of incompatible type const
# char *".
#
# Reporter hit this in action_controller/base.rb's
# `@location = location` (location is a `location: nil` kwarg)
# followed by `res.headers["Location"] = controller.location`
# at the Tep::Server entry. Same pattern, minimized here.
#
# Fix: compile_expr_as_string unboxes a poly value via `.v.s`
# so the typed-hash setter type-checks. Mirrors the existing
# symbol -> sp_sym_to_s coerce in the same helper.

class C
  attr_reader :loc

  def initialize
    @loc = nil
  end

  def render(loc: nil)
    @loc = loc
  end
end

class Bag
  attr_reader :h

  def initialize
    @h = {}
    @h["k"] = "v"
  end
end

c = C.new
c.render(loc: "/b")
b = Bag.new
b.h["k2"] = c.loc
puts b.h["k"]
puts b.h["k2"]
