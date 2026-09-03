# Array and Hash rendering: `to_s`/`print`/interpolation all use the bracketed
# inspect form; only `puts` flattens an array onto separate lines.

arr = [1, [2, "x"]]
p arr.to_s                    # "[1, [2, \"x\"]]"
puts arr.to_s                 # [1, [2, "x"]]
print arr                     # [1, [2, "x"]]
puts
puts "interp: #{arr}"         # interp: [1, [2, "x"]]

# `puts` (and only `puts`) flattens an array, one element per line.
puts [1, 2, 3]                # 1 / 2 / 3

# Hash to_s / interpolation inspect keys and values (a String value keeps its
# quotes).
h = { a: "x", 3 => [1] }
p h.to_s                      # "{a: \"x\", 3 => [1]}"
puts "h: #{h}"                # h: {a: "x", 3 => [1]}
puts h                        # {a: "x", 3 => [1]}

# A Range's to_s uses the plain (to_s) form of its bounds.
p (1..3).to_s                 # "1..3"
__END__
"[1, [2, \"x\"]]"
[1, [2, "x"]]
[1, [2, "x"]]
interp: [1, [2, "x"]]
1
2
3
"{a: \"x\", 3 => [1]}"
h: {a: "x", 3 => [1]}
{a: "x", 3 => [1]}
"1..3"
