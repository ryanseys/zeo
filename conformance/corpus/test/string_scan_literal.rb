# String#scan accepts a String pattern in both its value and block forms.
p "MixedCase".scan("e")
p "aaaa".scan("aa")
p "abc".scan("z")

pattern = "an"
p "banana".scan(pattern)

# An empty pattern matches at every character boundary, including both ends.
p "abc".scan("")
p "caf\u00e9".scan("")

# Exercise the statement-form iterator lowering.
statement_matches = []
"cababa".scan("ba") { |match| statement_matches << match }
p statement_matches

matches = []
source = "banana"
returned = source.scan(pattern) { |match| matches << match.upcase }
p matches
p returned == source
