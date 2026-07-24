# #562 (gurgeous / Adam Doppelt). A corpus of `<container_op>
# == nil` predicates that CRuby answers as true but spinel
# previously emitted as false. Root cause: typed-array /
# typed-hash returns can't represent nil at the value level
# (sp_IntArray_get returns 0 on out-of-range, sp_StrIntHash_get
# returns 0 on missing key, etc.), so the existing
# `int == nil` short-circuit in compile_eq folded the
# predicate to FALSE.
#
# compile_container_op_nil_check recognizes specific call
# shapes on the lhs of `== nil` and emits a CRuby-equivalent
# runtime predicate:
#   arr[i] / arr.delete_at(i) -> (i < 0 || i >= len)
#   arr.first / arr.last / arr.pop -> (len == 0)
#   arr.find_index(x) -> !arr.include?(x)
#   hash[k] -> !hash.has_key?(k)
#   regex.match(s) -> !regex.match?(s)
#
# Plus `Array.new.instance_of?(Array)` folds to TRUE: every
# Array variant (int_array / str_array / etc.) is
# conceptually `Array`; same for Hash variants.

p ([1, 2, 3][99] == nil)          # 01
p Array.new.instance_of?(Array)   # 02
p ([1, 2].delete_at(3) == nil)    # 03
p ([].first == nil)               # 04
p ([].last == nil)                # 05
p ([].pop == nil)                 # 06
p ([1, 2].find_index(3) == nil)   # 07
p ({0 => 0}[5] == nil)            # 08
p (/xyz/.match("abxyc") == nil)   # 09
