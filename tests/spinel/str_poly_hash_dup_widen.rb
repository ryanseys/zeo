# A local declared `sp_StrPolyHash *` (because later poly-value writes
# force the wider variant) initialized from `<str_str_hash>.dup`
# previously emitted a raw `lv = sp_StrStrHash_dup(...)` assignment.
# The two structs have different `vals[]` element types
# (`const char **` vs `sp_RbVal *`), so subsequent reads through the
# widened slot returned garbage (0 / nil) even when -Werror was off.
# The prior `.length`-only assertion happened to land at an offset
# both structs share. Issue #614.

class Match
  attr_reader :path_params
  def initialize(pp); @path_params = pp; end
end

m = Match.new({ "article_id" => "1", "format" => "html" })
merged = m.path_params.dup
merged["x"] = 1        # int value forces analyzer-side widening to str_poly_hash
puts merged["article_id"]   # round-trips the original str value
puts merged["format"]
puts merged["x"]
puts merged.length
