# #456: `outer[k1][k2] = v` where outer's value type is sp_RbVal
# (StrPolyHash). The inner `[]=` recv is poly; with no
# `is_a?(Hash)` narrowing, the dispatch needs to enumerate Hash-
# storage arms, not only Array-storage ones. Prior to this fix
# the poly-recv `[]=` codegen emitted PolyArray / PtrArray /
# IntArray arms only and the call failed C compile with
# "passing argument 2 of sp_PolyArray_set makes integer from
# pointer without a cast" when the index was a string literal.
#
# Surfaced in the canonical form-decoder shape
# `into[outer][inner] = val` inside `CgiIo.assign_form_pair`.

def assign(into)
  into["x"]["y"] = "z"
  into["x"]["y"]
end

box = { "x" => { "y" => "old" } }
puts assign(box)
