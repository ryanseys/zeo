# An array pattern asks `#deconstruct` at run time; a class without
# one simply does not match (no raise), so the `else` arm runs --
# see `clif::patterns`' array-pattern lowering.

class Plain
end
o = Plain.new
case o
in [a, b]
  puts "matched"
else
  puts "no deconstruct, fell to else"
end
__END__
no deconstruct, fell to else
