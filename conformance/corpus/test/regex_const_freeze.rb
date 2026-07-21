# Issue #394: a top-level `PATTERN = /regex/.freeze` constant
# was not recognised as a regex source. `gsub(PATTERN, ...)` then
# fell through to `sp_str_gsub(... cst_PATTERN ...)` (mrb_int -> char *
# C-compile error). The non-`.freeze` form already worked because
# find_regexp_index recursed through @const_expr_ids; .freeze
# wraps the literal in a CallNode so the recursion missed.
#
# Fix: find_regexp_index now peels the `.freeze` CallNode wrapper
# before checking for RegularExpressionNode.

PATTERN1 = /abc/
PATTERN2 = /xy[0-9]+/.freeze
PATTERN3 = /^foo/.freeze

puts "xabcyz".gsub(PATTERN1, "Q")        # "xQyz"
puts "xy42 xy7".gsub(PATTERN2, "M")      # "M M"
puts "foobar foobaz".gsub(PATTERN3, "Y") # "Ybar foobaz"
