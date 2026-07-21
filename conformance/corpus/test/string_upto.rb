# String#upto walks successive strings (the same succ-based sequence a
# String range produces) from the receiver to the argument inclusive,
# yielding each to the block. The block-less form materializes the
# sequence as an array. Previously codegen treated the receiver as an
# integer loop variable and failed to compile.
count = 0
"a".upto("e") { |c| count += 1 }
puts count

out = ""
"x".upto("z") { |c| out += c }
puts out

# multi-character carry
multi = ""
"ay".upto("bb") { |c| multi += c + "," }
puts multi

# block-less form materializes to an array
puts "a".upto("c").to_a.join("-")
