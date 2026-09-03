# THE keystone parity test: `.ancestors` for every core class, byte-
# identical to real ruby. (Mutex/Queue excluded: real Ruby names them
# `Thread::Mutex`/`Thread::Queue` -- a documented naming divergence.)

[BasicObject, Object, Kernel, Comparable, Enumerable,
 Numeric, Integer, Float, Rational, Complex,
 String, Symbol, Array, Hash, Range,
 NilClass, TrueClass, FalseClass,
 Proc, Regexp, MatchData, Struct, Enumerator,
 Class, Module, Math, Fiber, Thread, Ractor].each do |c|
  puts "#{c}: #{c.ancestors.inspect}"
end
__END__
BasicObject: [BasicObject]
Object: [Object, Kernel, BasicObject]
Kernel: [Kernel]
Comparable: [Comparable]
Enumerable: [Enumerable]
Numeric: [Numeric, Comparable, Object, Kernel, BasicObject]
Integer: [Integer, Numeric, Comparable, Object, Kernel, BasicObject]
Float: [Float, Numeric, Comparable, Object, Kernel, BasicObject]
Rational: [Rational, Numeric, Comparable, Object, Kernel, BasicObject]
Complex: [Complex, Numeric, Comparable, Object, Kernel, BasicObject]
String: [String, Comparable, Object, Kernel, BasicObject]
Symbol: [Symbol, Comparable, Object, Kernel, BasicObject]
Array: [Array, Enumerable, Object, Kernel, BasicObject]
Hash: [Hash, Enumerable, Object, Kernel, BasicObject]
Range: [Range, Enumerable, Object, Kernel, BasicObject]
NilClass: [NilClass, Object, Kernel, BasicObject]
TrueClass: [TrueClass, Object, Kernel, BasicObject]
FalseClass: [FalseClass, Object, Kernel, BasicObject]
Proc: [Proc, Object, Kernel, BasicObject]
Regexp: [Regexp, Object, Kernel, BasicObject]
MatchData: [MatchData, Object, Kernel, BasicObject]
Struct: [Struct, Enumerable, Object, Kernel, BasicObject]
Enumerator: [Enumerator, Enumerable, Object, Kernel, BasicObject]
Class: [Class, Module, Object, Kernel, BasicObject]
Module: [Module, Object, Kernel, BasicObject]
Math: [Math]
Fiber: [Fiber, Object, Kernel, BasicObject]
Thread: [Thread, Object, Kernel, BasicObject]
Ractor: [Ractor, Object, Kernel, BasicObject]
