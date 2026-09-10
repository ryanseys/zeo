# `C.frozen?` is false for an ordinary class.
class C001; end
p(C001.frozen?)
__END__
false
