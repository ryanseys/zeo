# A GNU statement-expression's value is its last statement only when that
# statement is an EXPRESSION. The statement form of a `rescue` modifier is an
# if/else over setjmp whose two arms compute the value and drop it, so a block
# whose tail is one produced a void:
#
#   error: void value not ignored as it ought to be
#
# The if / case / begin tails were already on the list it was missing from.
def wrap
  p(yield)
end

wrap { 1 rescue 9 }
wrap { (raise "x") rescue 9 }
wrap { Integer("101", 2) rescue 9 }
wrap { Integer("nope") rescue -1 }

def twice
  [yield, yield]
end
p(twice { 2 rescue 0 })

p [1, 2, 0].map { |d| 10 / d rescue :div0 }
p ["1", "x"].map { |s| Integer(s) rescue nil }

# and the tails that already worked, unchanged
wrap { if true then 1 else 2 end }
wrap { case 1 when 1 then "one" else "other" end }
wrap { begin; 5; end }
