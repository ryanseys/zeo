# `each { |num, sym| }` over an array of pairs binds both halves, and the string built from one of them.
table = [[1000, "M"], [4, "IV"], [1, "I"]]
out = ""
table.each { |num, sym| out += sym }
p out
__END__
"MIVI"
