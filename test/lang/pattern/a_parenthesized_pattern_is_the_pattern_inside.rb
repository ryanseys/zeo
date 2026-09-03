# `(A | B)` in pattern position is just the alternation inside -- prism
# wraps it in a ParenthesesNode, and the pattern walk must unwrap it both
# where it stands alone and where a capture binds it.
#
# zeo's pattern lowering had no parentheses arm, so the node fell through
# to the generic expression rejection ("unsupported syntax at \"Integer |
# String\"") -- anthropic's credential reader and jpt's path parser both
# spell it this way.
config = { expires_in: 300 }
case config
in { expires_in: (Integer | String) => expires_in }
  puts "expires: #{expires_in}"
end

segments = ["@", "name", "first"]
case segments
in [("@" | "$"), *rest]
  puts "anchored, rest=#{rest.inspect}"
end

case 42
in (Integer | Float)
  puts "numeric"
end
__END__
expires: 300
anchored, rest=["name", "first"]
numeric
