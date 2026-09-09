# `String#capitalize` maps the Unicode digraphs to their TITLECASE forms
# (U+01F3 dz -> U+01F2 Dz, and the Lj/Nj/Dz families), and #swapcase of a
# titlecase char lowercases per-half ("Dz" -> "dZ"). zeo maps both
# through the full-uppercase table. #upcase and #downcase already match;
# only the titlecase third of the case-mapping triple is missing.
p "ǳ".capitalize
p ["ǉx".capitalize, "ǌx".capitalize, "ǆx".capitalize]
p "ǲ".swapcase
p "ǳ".upcase
p "ǲ".downcase
__END__
"ǲ"
["ǈx", "ǋx", "ǅx"]
"dZ"
"Ǳ"
"ǳ"
