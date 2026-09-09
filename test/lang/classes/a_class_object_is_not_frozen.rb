# `C.frozen?` is false for an ordinary class.
# (spinel issue #2953)
class C001; end
p(C001.frozen?)
__END__
false
