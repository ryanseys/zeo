# `parameters(lambda:)` forces the view: true reports plain positionals as
# :req, false as :opt, nil follows the receiver. A defaulted positional
# stays :opt in every view; a bound method's to_proc keeps the method arity.

p proc { |x, y| }.parameters(lambda: true)
p ->(x, y) { }.parameters(lambda: false)
p proc { |x, y = 1| }.parameters(lambda: true)
p proc { |x| }.parameters(lambda: nil)
p lambda { _1 }.parameters
def greet(name); end
gp = method(:greet).to_proc
p [gp.arity, gp.lambda?]
__END__
[[:req, :x], [:req, :y]]
[[:opt, :x], [:opt, :y]]
[[:req, :x], [:opt, :y]]
[[:opt, :x]]
[[:req, :_1]]
[1, true]
