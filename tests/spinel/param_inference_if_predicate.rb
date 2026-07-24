# Issue #314 (A): `infer_cls_meth_param_from_body`'s walker missed
# IfNode predicates and else-branches when collecting param method
# usages, leaving the param at the "int" default and routing
# `<param>.method(...)` accesses through the int-class fallback —
# which silently picks the first user class that defines the
# method and casts the int to that class's struct pointer.
#
# Body shape:
#   def update(p)
#     if p.title.nil?      # predicate uses p — was invisible
#       nil
#     else
#       self.title = p.title   # else-branch uses p — was invisible
#     end
#   end
#
# `collect_param_methods` walked @nd_body but not @nd_predicate /
# @nd_subsequent / @nd_else_clause. With ONLY `p.title` inside an
# `if/else`, called == [] and the param-from-body inference skipped
# the method entirely.
#
# Fix: extend the walk to predicate, subsequent, else_clause,
# collection (for-in), block, elements, parts, conditions
# (case/when). Now `called=["title"]` is collected and
# `class_has_all_methods` picks a class that defines `title`.
#
# Surfaced via Roundhouse's emitted real-blog where `Article#update
# (p)`'s body has exactly this shape and the resulting `mrb_int lv_p`
# routed `p.title` and `p.body` to two different user classes
# (ArticleRow for one, ActionResponse for the other) — same `lv_p`
# cast to two different struct pointers, structurally invalid C.

class P
  def title
    @title
  end
  def title=(v)
    @title = v
  end
  def body
    @body
  end
  def body=(v)
    @body = v
  end
end

class App
  # `p` is used ONLY inside the if-predicate (`p.title.nil?`),
  # nowhere in the IfNode's body or else-branch. Pre-fix the
  # collect_param_methods walker missed @nd_predicate /
  # @nd_subsequent / @nd_else_clause, so called == [] and the
  # param-from-body inference left `p` at "int". The
  # int-class fallback then routed `p.title` through
  # `((sp_<first-class-with-title> *)lv_p)->iv_title` — picking
  # whichever user class happened to be declared first that has
  # a `title` method. In Roundhouse that's a wildly different
  # struct than the actual `ArticleParams` the call site passes.
  def status(p)
    if p.title.nil?
      "no-title"
    else
      "has-title"
    end
  end
end

# IMPORTANT: don't call `App#status` directly — the call-site
# widening pass would pin `p` to obj_P and mask the body-walker
# bug. The test verifies that body-walker-derived inference fires
# even without a call site, by ensuring the C compiles cleanly
# despite multiple user classes (P, Other) that define `title`.

class Other
  def title; "wrong"; end
end

# Pin both classes' instances at the top level so they appear in
# the symbol table.
o = Other.new
puts o.title             # wrong

q = P.new
q.title = "hello"
puts q.title             # hello

# `App#status` is defined but never called — purely body-walker
# determines `p`'s type. Pre-fix: left at "int", emitted broken C
# routing through whichever class came first.
puts "compiled-cleanly"
