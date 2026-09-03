# The cycle this program builds on purpose, and the census the
# `ZEO_RT_GCCHECK=1` leg gates against. A cycle alive at exit is not a
# defect: it is a ring the program never broke. What the leg gates is a
# CHANGE to the line below.
# 
# PrettyPrint's own graph. A Group holds its GroupQueue and the queue holds the group back.
#@ gccheck: cycle leak: 219 objects (Array x155, PP x16, PrettyPrint::Group x16, PrettyPrint::GroupQueue x16, Proc x16)
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
__END__
{a: 1}
{a: 1, b: 2}
{"k" => "v"}
[1, 2, 3]
"str"
42
nil
{a: {b: {c: 1}}}
[{x: 1}, {y: 2}]
"{a: 1}\n"
"[1, [2, [3]]]\n"
"{a: 1}\n"
"{a: 1, b: 2}"
{"a b": 1}
{1 => :one}
"#<Plain:ADDR @x=1>\n"
"[1,\n 2,\n 3,\n 4,\n 5,\n 6,\n 7,\n 8,\n 9,\n 10,\n 11,\n 12,\n 13,\n 14,\n 15,\n 16,\n 17,\n 18,\n 19,\n 20,\n 21,\n 22,\n 23,\n 24,\n 25,\n 26,\n 27,\n 28,\n 29,\n 30,\n 31,\n 32,\n 33,\n 34,\n 35,\n 36,\n 37,\n 38,\n 39,\n 40]\n"
true
"global-variable"
