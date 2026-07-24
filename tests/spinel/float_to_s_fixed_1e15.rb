# Float#to_s uses plain decimal (not scientific) for a fractional value whose
# integer part is 16 digits (1e15..1e16), matching CRuby (#2593).
puts 4503599627370495.5
puts 1000000000000000.5
puts 1234567890123456.7
puts 123456789012345.6
# integer-valued 16-digit doubles still print scientific
puts 9007199254740992.0
puts 1e15
puts 1e16
