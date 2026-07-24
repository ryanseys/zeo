# String#tr / tr_s with ^-negation in from-set
# ^ at the start of from-set inverts which chars get translated.

# tr with ^-negation
puts "abc".tr("^a", "x").inspect
puts "hello".tr("^aeiou", ".").inspect
# tr without negation (positional mapping)
puts "hello".tr("el", "ip").inspect
# tr_s with ^-negation
puts "hello".tr_s("^aeiou", ".").inspect
