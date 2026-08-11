# The `alias` KEYWORD with an interpolated symbol -- `alias :"#{kind}_attr"
# :"#{kind}_attrs"` -- has no compile-time spelling, but it is still just a
# runtime install on the default definee, in class-body order.
#
# zeo required a plain symbol and refused the whole file ("`alias`'s target
# must be a plain method name") -- formal_wear builds its DSL in exactly
# this loop.
class Suit
  def tie_attrs
    ["windsor"]
  end

  kind = "tie"
  alias :"#{kind}_attr" :"#{kind}_attrs"
end

puts Suit.new.tie_attr.inspect

# Top level: the same install onto Object.
def given
  "given!"
end

adverb = "when"
alias :"#{adverb}_step" :given
puts when_step
