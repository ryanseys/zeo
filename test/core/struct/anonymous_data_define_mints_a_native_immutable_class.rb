# `Data.define` outside a constant assignment mints a native immutable data
# class -- keyword construction, `with`, frozen, byte-exact `inspect`.

d = Data.define(:x, :y)
pt = d.new(x: 1, y: 2)
p pt
p pt.x
p pt.to_h
p pt.with(x: 9)
p pt.frozen?
__END__
#<data x=1, y=2>
1
{x: 1, y: 2}
#<data x=9, y=2>
true
