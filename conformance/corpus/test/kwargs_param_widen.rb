# Issue #314 (B): a module class method's required keyword params
# never widened from the "int" default at scan_new_calls time. The
# AST presents kwargs as a single trailing KeywordHashNode, and the
# existing positional-only widening loop unified the hash type into
# `ptypes[0]` and never touched the per-kwarg slots.
#
# Two complementary fixes:
#  1. widen_ptypes_from_args helper that walks KeywordHashNode
#     children, mapping each `key: value` pair to the matching
#     keyword param's slot in the callee's ptypes array.
#  2. compile_call_args_with_defaults / compile_call_args_splat
#     emit-side: replace the hardcoded `sp_box_str(...)` for poly
#     kwargs with `box_expr_to_poly(...)` so the box helper is
#     picked from the arg's actual source type.
#
# Surfaced via Roundhouse's emitted real-blog `form_with(model:,
# model_name:, action:, method:, opts:)` shape — minimal repro
# extracted with two distinct obj types passed to the same kwarg.

# Article/Comment have an array ivar so neither qualifies for the
# value-type optimization — instances stay heap-allocated and the
# captured-by-poly boxing site (`sp_box_obj`) sees a real pointer.
# (Pure-int/string-ivar classes flowing into a poly param at a call
# site is a separate, orthogonal value-type-exclusion concern that
# surfaces only with inline `X.new(...)` directly inside a poly-
# typed call slot — not exercised by this issue's Roundhouse trigger,
# where the receiver is always a previously-allocated local.)

class Article
  def initialize(id, name); @id = id; @name = name; @tags = []; end
  def kind; "article-" + @name; end
end

class Comment
  def initialize(body); @body = body; @replies = []; end
  def kind; "comment-" + @body; end
end

module M
  def self.render(model:, label:)
    "[#{label}] #{model.kind}"
  end
end

# Pre-allocate (matches the Roundhouse `lv_article` shape — locals
# carry `sp_<C> *`, not struct-by-value).
a = Article.new(7, "alpha")
c = Comment.new("hello")

# Two call sites with different obj types for `model:`. The widening
# pass must collapse them to poly so the call-site box helper picks
# sp_box_obj (not the pre-fix sp_box_str).
puts M.render(model: a, label: "art")     # [art] article-alpha
puts M.render(model: c, label: "com")     # [com] comment-hello

# Module-inner class constructor inside a module class method
# body. infer_function_body_call_types now pins
# @current_method_name so resolve_const_read_name's
# `<Mod>_cls_<m>` peel resolves `Inner` to `M_Inner` — pre-fix
# the find_class_idx lookup returned -1 and skipped the
# constructor-arg widening of N::Inner.@x.
module N
  class Inner
    def initialize(x); @x = x; end
    def show; "inner=#{@x.kind}"; end
  end

  def self.make(model:)
    Inner.new(model).show
  end
end

puts N.make(model: a)                     # inner=article-alpha
