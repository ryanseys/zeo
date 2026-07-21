# Exception classes as first-class constants: defined?, superclass chain,
# and the rarer subclasses as reflectable values.
p defined?(ArgumentError)
p defined?(StopIteration)
p StopIteration.superclass
p SyntaxError.superclass
p Interrupt.superclass
p SignalException.superclass
p NoMatchingPatternKeyError.superclass
p ClosedQueueError.superclass
p UncaughtThrowError.superclass
p EOFError.superclass
p ThreadError.superclass
p FiberError.name
p SecurityError.superclass
p RegexpError.superclass
p EncodingError.superclass
p NoMatchingPatternError.superclass
