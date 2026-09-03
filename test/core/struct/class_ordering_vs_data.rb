P = Data.define(:x, :y)
S = Struct.new(:a)
p(P < Data)
p(S < Struct)
p(P < Struct)
__END__
true
true
nil
