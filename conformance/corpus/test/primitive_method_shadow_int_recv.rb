# Issue #305: follow-up to #302's primitive-shadow fix.
#
# #302 stopped the body-side param-type inference from committing
# a param to a user class when the called methods are also defined
# on a primitive type (`def find_bracket(s); s.index("[")` no
# longer pins `s` as obj_StringIndexer when the only call site
# passes a string literal).
#
# But the *emit-time* int-recv fallback (`compile_int_class_fallback
# _expr`) still walked every user class and picked the first one
# whose method-name matched. When the receiver flowed in as
# `mrb_int` (e.g. unpinned param, IntArray-of-pointer fallback),
# the fallback would route `raw_key.index("[")` to
# `ArticlesController#index` regardless of the receiver's actual
# nature.
#
# Fix: when `mname` is a primitive-shared method AND any user class
# defines it, refuse the int-to-class cast and let the unresolved-
# call diagnostic fire. The shape happens to compile to a `0`
# placeholder for the call's result — that's the existing fallback
# contract for genuinely unresolvable calls; it's vastly better
# than the silent miscompile that picked a random user class.

class ArticlesController
  def index
    "controller-action"
  end
end

# A param with no upstream pinning. Spinel defaults the param to
# mrb_int when no call site supplies a non-int. Without the fix,
# the body's `.index` call hits compile_int_class_fallback_expr,
# which walks the user-class table and picks ArticlesController
# (the first class defining `def index`), emitting a typed cast
# `((sp_ArticlesController *)raw_key)->index_get(...)` that is
# either a C-compile error or a silent miscompile. With the fix,
# the int-recv fallback refuses primitive-shared method names and
# the call lowers to the unresolved-call placeholder (literal 0).
#
# We don't call `find_first` from Ruby — defining it is enough for
# spinel to emit and type-check the body, and avoiding the call
# keeps MRI happy (Integer#index doesn't exist).
def find_first(raw_key)
  raw_key.index("[")
end

puts "compiled-cleanly"
