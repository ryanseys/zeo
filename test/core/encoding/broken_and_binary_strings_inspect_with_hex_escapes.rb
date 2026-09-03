# A byte that isn't a character in the string's encoding renders as
# \xNN, and the cross-encoding equality rule keeps ASCII-only strings
# equal across encodings. Cross-checked against ruby 4.0.6.

p "abc".b == "abc"
p "abc".b.encoding
s = "\xff\x80".b
p s.valid_encoding?
puts s.inspect
puts "caf\xe9".force_encoding("ISO-8859-1").inspect
__END__
true
#<Encoding:BINARY (ASCII-8BIT)>
true
"\xFF\x80"
"caf\xE9"
