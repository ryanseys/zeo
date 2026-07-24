# `case v when X then ... end` bound to a local via `r = ...`
# pre-fix emitted two C temps: a result temp (typed from the
# branch values) and a scrutinee temp (used inside
# `sp_class_for_poly(...)`). The scrutinee temp wrongly inherited
# the result type. For a poly scrutinee with int result, the
# emit produced `mrb_int _t2 = lv_v;` (a value-conversion that
# discards the sp_RbVal struct), then `sp_class_for_poly(_t2)`
# failed C compile because the helper expects sp_RbVal.
#
# Fix: the scrutinee temp picks its C type from the predicate's
# inferred type (not from the case-result type). Added a `poly`
# arm to compile_expr's CaseNode emit so `sp_RbVal _t2 = lv_v;`
# lands correctly. The compile_case_stmt sibling arm gains
# concrete `range` / `time` / `float` / `bool` arms too so a
# tail-position `case <range>` doesn't fall to the int default.
# Issue #604.

def classify(v)
  r = case v when Range then 1 end
  r
end
puts classify(200..299)             # 1 (Range matches)
puts classify(404)                  # 0 (no when match -> default 0)

# Case-when result bound to local with mixed branches and poly scrutinee.
def label(v)
  r = case v
      when Range   then "range"
      when Integer then "int"
      else "other"
      end
  r
end
puts label(200..299)                # range
puts label(404)                     # int
puts label("hi")                    # other
