# The chars/split result of a String destructured from a pair, captured (map /
# returned): the auto-splat must unbox the poly element into the String param's
# const char* slot, not leave it a raw sp_RbVal.
r = [["ab", "cd"]].map { |a, b| a.chars }
p r
p [["a,b", "c"]].map { |a, b| a.split(",") }
p [["ab", "cd"]].map { |a, b| a.chars.length }
p [[:x, :y]].map { |a, b| a.to_s }
