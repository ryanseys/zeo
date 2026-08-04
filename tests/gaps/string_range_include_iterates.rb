# `Range#include?` WALKS the range; `Range#cover?` compares endpoints with
# `<=>`. For a String range the two disagree, and zeo answers `cover?` for both.
#
# `("a".."e").include?("bb")` is false -- `bb` is not one of a, b, c, d, e --
# while `cover?("bb")` is true, because `"a" <= "bb" <= "e"`. zeo aliases
# `include?`/`member?` onto `===`'s cover check
# (`builtins::range.rs`'s shared row), whose comment claims the approximation is
# faithful "for the numeric/comparable cases this runtime supports". A String
# range is exactly the case it is not faithful for, and ruby's own docs use this
# pair as the illustration of the difference.
#
# Ruby takes the cover shortcut only for a range whose endpoints are Numeric
# (`range.c`'s `range_include_internal` falls through to `rb_ary_includes`-style
# iteration otherwise), so the numeric answers below must stay as they are.

p ("a".."e").include?("bb")
p ("a".."e").cover?("bb")
p ("a".."e").member?("bb")
p ("a".."e") === "bb"

p ("a".."e").include?("c")
p ("a".."z").include?("cc")

# Numeric ranges take the shortcut in ruby too -- these must not change.
p (1..10).include?(5.5)
p (1..10).cover?(5.5)
