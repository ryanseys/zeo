# `::Integer === 7` and friends, matching and not.
p(::Integer === 7)
p(::Integer === "x")
p(::String === "a")
p(::Float === 1.5)
p(::Symbol === :s)
__END__
true
false
true
true
true
