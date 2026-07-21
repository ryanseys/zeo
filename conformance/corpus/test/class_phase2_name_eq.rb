# Issue #404 Phase 2: `.name`, `.inspect`, `.==`, `.!=`, `.eql?`
# on a sp_Class value. Phase 1 wired `.to_s` and the class-as-value
# round-trip through locals / ivars / hash values. Phase 2 fills
# in the remaining "simple" reflection surface that doesn't need
# precomputed ancestors:
#
#   - `.name` / `.inspect` alias `.to_s` (sp_class_to_s lookup).
#     CRuby's #inspect for a class is technically "Foo" too -- it's
#     `<Class:Foo>` only via the Object#inspect fallback when the
#     class doesn't override #inspect (which it does, returning
#     just the name). Phase 2 keeps the bare-name shape; the
#     "#<Class:Foo>"-ish polish is Phase 3 work if ever needed.
#   - `.==` / `.eql?` -- sp_class_eq(a, b) (cls_id field compare).
#   - `.!=` -- the negation thereof.
#
# Out of scope: `.superclass`, `.ancestors`, `< / <= / > / >=`,
# `case <obj> when <Class>`, dynamic `is_a?(c)` against a
# variable. Those need the precomputed ancestor table from
# docs/CLASS-OBJECT.md.
#
# Coverage:
#   - .name / .inspect on a class literal and through obj.class.
#   - == / != / eql? between class literals, mixing with subclass,
#     and through obj.class chains.
#   - Subclass-of-Base still gets the subclass cls_id from .class
#     (regression check on #419's static dispatch).

class Row
  attr_accessor :id
  def initialize
    @id = 0
  end
end

class Article < Row
end

# Direct class literal.
puts Row.name                 # Row
puts Article.inspect          # Article

# Through obj.class.
r = Row.new
a = Article.new
puts r.class.name             # Row
puts a.class.inspect          # Article

# Equality / inequality / eql?
puts (Row == Row).to_s        # true
puts (Row == Article).to_s    # false
puts (Row != Article).to_s    # true
puts (Row != Row).to_s        # false
puts Row.eql?(Row).to_s       # true
puts Row.eql?(Article).to_s   # false

# Through obj.class chains.
puts (r.class == Row).to_s            # true
puts (r.class == Article).to_s        # false
puts (a.class != Row).to_s            # true
puts (a.class == Article).to_s        # true
puts r.class.eql?(Row).to_s           # true
