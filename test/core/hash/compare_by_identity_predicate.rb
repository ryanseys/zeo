# Hash#compare_by_identity? reports false for an ordinary value-keyed hash,
# whatever shape the hash was built in. The mutating `compare_by_identity`
# has its own tests beside this one.
def s(x); x; end
p s({}).compare_by_identity?
p s({ "a" => 1 }).compare_by_identity?
h = {}
p h.compare_by_identity?
__END__
false
false
false
