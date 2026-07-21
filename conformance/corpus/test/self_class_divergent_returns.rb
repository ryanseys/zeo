# Issue #431 (#422 residual). `self.class.<cmeth>` dispatch in a
# parent-defined instance method now lowers to a cls_id switch
# even when the base and subclass overrides have divergent
# return types -- the canonical Rails "abstract base raises,
# subclass returns concrete value" pattern.
#
# Pre-fix: #422's switch was gated on all candidate owners
# returning the same type. When Base.label raised (no return
# value, default int) and Article.label returned a String,
# the gate failed and the dispatch fell through to a direct
# call to sp_Base_cls_label -- which raised even when self
# was statically a subclass instance whose override was the
# intended target.
#
# Fix shape:
#   - codegen: divergent-types path boxes each arm's return
#     to sp_RbVal so the result temp has a single C type;
#     base call lands in `default:` so it only runs when the
#     runtime cls_id genuinely is the base.
#   - analyze: infer_type for `<obj>.class.<m>` chain widens
#     to "poly" when descendants override with divergent
#     return types, so downstream callers route through the
#     poly-dispatch path.
#
# Coverage:
#   - Variant 1: Base.label raises, Article.label returns
#     String. Article.new.describe should print the String.
#   - Variant 2: Base.foo returns int, Article.foo returns
#     String. Both instance routes return through the cls_id
#     switch (Article gets the String, Base gets the int via
#     the default arm). Both render correctly via puts on the
#     boxed sp_RbVal.

class Base
  def self.label
    raise "subclass must override"
  end

  def describe
    self.class.label
  end
end

class Article < Base
  def self.label
    "articles"
  end
end

puts Article.new.describe

class IntBase
  def self.foo; 42; end
  def show; self.class.foo; end
end
class StrChild < IntBase
  def self.foo; "child"; end
end

puts StrChild.new.show
puts IntBase.new.show
