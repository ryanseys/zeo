# The message names the value and the sub-test that rejected it,
# which is the whole diagnostic for a `case/in` -- oracle-verified.

case 5
in String
  puts "no"
end
__END__
#@ stderr
lang/exceptions/case_in_with_no_matching_arm_and_no_else_raises.rb:4:in '<main>': 5: String === 5 does not return true (NoMatchingPatternError)
#@ exit 1
