# A Hash replacement in #sub/#gsub answers its default for a key it does not
# have; the empty string was hard-coded (#3824).
h = Hash.new("?")
h["e"] = "3"
p "hello".sub(/l/, h)
p "hello".sub(/e/, h)
p "hello".gsub(/l/, h)
g = { "e" => "3" }
p "hello".sub(/l/, g)
p "hello".sub(/e/, g)
__END__
"he?lo"
"h3llo"
"he??o"
"helo"
"h3llo"
