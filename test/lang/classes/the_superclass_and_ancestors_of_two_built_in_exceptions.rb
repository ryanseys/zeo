# Interrupt and RuntimeError each report their own parent and full chain.
# (spinel issue #3022)
p Interrupt.new.class.superclass
p Interrupt.new.class.ancestors
p RuntimeError.new.class.superclass
p RuntimeError.new.class.ancestors
__END__
SignalException
[Interrupt, SignalException, Exception, Object, Kernel, BasicObject]
StandardError
[RuntimeError, StandardError, Exception, Object, Kernel, BasicObject]
