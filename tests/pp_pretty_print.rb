# `pp` -- the gap this closes was three separate defects, each of which the
# pretty printer walks straight into:
#
#   * `$>` read nil, where it is not a copy of `$stdout` but the SAME slot, and
#     `PP.pp` defaults its output to it;
#   * `yield(*v, **kwsplat)` passed a stray `{}` when the keywords turned out
#     empty, so `seplist`'s `|k, v|` bound the whole pair to `k`;
#   * a `def` in each branch of a `RUBY_VERSION` guard left two definitions of
#     one name in the body and the later simply won, so the pre-3.4 hash
#     renderer was the one that ran.
require "pp"

pp({a: 1})
pp({a: 1, b: 2})
pp({"k" => "v"})
pp [1, 2, 3]
pp "str"
pp 42
pp nil
pp({a: {b: {c: 1}}})
pp [{x: 1}, {y: 2}]

# The same rendering, captured rather than printed.
p PP.pp({a: 1}, +"")
p PP.pp([1, [2, [3]]], +"")
p({a: 1}.pretty_inspect)
p PP.singleline_pp({a: 1, b: 2}, +"")

# A symbol key needing quotes takes the `=>` form even in the 3.4 renderer.
pp({:"a b" => 1})
pp({1 => :one})

# An object with no `pretty_print` of its own falls back to its inspect.
class Plain
  def initialize = @x = 1
end
p PP.pp(Plain.new, +"").sub(/0x[0-9a-f]+/, "ADDR")

# Wide enough to break across lines, which is the whole point of the group
# machinery underneath.
p PP.pp((1..40).to_a, +"", 40)

# `$>` is one slot with `$stdout`, which is where `PP.pp` sends its output by
# default.
p $>.equal?($stdout)
p defined?($>)
