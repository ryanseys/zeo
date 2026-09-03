# `Class#allocate` answers a blank instance for every native class ruby
# allocates, and refuses for every one ruby refuses -- with ruby's own error
# class each time.
#
# 30 of these 52 rows used to diverge. Only 2 of zeo's 98 native classes
# declared a blank value, so `allocate` fell through to a refusal for the
# other 26 that ruby allocates, and `Marshal.load` -- which allocates before
# it fills -- could not load any of them.
#
# The three shapes ruby uses, and now zeo too:
#
#   * a blank instance, for a class whose payload has an empty form;
#   * `TypeError: allocator undefined for X`, for a class with no blank;
#   * `NoMethodError: undefined method 'allocate' for class X`, for the five
#     ruby UNDEFINES the name on (`zeo_abi::ALLOCATE_UNDEFINED`). The two
#     refusals are not interchangeable: `rescue TypeError` around
#     `MatchData.allocate` catches nothing.

%w[date set stringio strscan pathname time bigdecimal zlib socket monitor
   objspace digest etc].each { |f| require f }

NAMES = %w[
  Array Hash String Range Regexp MatchData Object BasicObject Rational Complex
  Struct Data Time Date DateTime Set StringIO StringScanner Pathname BigDecimal
  Dir File IO Encoding Enumerator Enumerator::Lazy Method UnboundMethod Proc
  Binding Module Class Thread Mutex Queue SizedQueue ConditionVariable
  ThreadGroup Fiber Random ObjectSpace::WeakMap Digest::MD5 Zlib::Deflate
  Zlib::Inflate Socket TCPSocket Etc::Passwd Process::Status Exception
  RuntimeError Errno::ENOENT TracePoint Symbol Integer Float
]

NAMES.each do |n|
  k = Object.const_get(n)
  row = begin
          # `BasicObject` has no `#class`, so name the class we asked rather
          # than the one the instance reports.
          k.allocate
          "ok"
        rescue ScriptError, StandardError => e
          "#{e.class}: #{e.message}"
        end
  puts "#{n}\t#{row}"
end

# The undef is INHERITED, because ruby's sits on the singleton chain.
sub = Class.new(MatchData)
begin
  sub.allocate
  puts "subclass\tallocated"
rescue NoMethodError => e
  puts "subclass\t#{e.class}"
end

# And it is a genuinely absent method, not a raising one.
puts "respond_to?\t#{[Rational, Complex, MatchData, Module].map { |k| k.respond_to?(:allocate) }.uniq.inspect}"

# A blank reports its own class, including where the payload alone could not
# say which one it is (`File` and the sockets share `IO`'s).
puts "classes\t#{[File, IO, Socket, TCPSocket, Date, DateTime, Queue, SizedQueue].map { |k| k.allocate.class }.inspect}"
__END__
Array	ok
Hash	ok
String	ok
Range	ok
Regexp	ok
MatchData	NoMethodError: undefined method 'allocate' for class MatchData
Object	ok
BasicObject	ok
Rational	NoMethodError: undefined method 'allocate' for class Rational
Complex	NoMethodError: undefined method 'allocate' for class Complex
Struct	TypeError: allocator undefined for Struct
Data	TypeError: allocator undefined for Data
Time	ok
Date	ok
DateTime	ok
Set	ok
StringIO	ok
StringScanner	ok
Pathname	ok
BigDecimal	TypeError: allocator undefined for BigDecimal
Dir	ok
File	ok
IO	ok
Encoding	TypeError: allocator undefined for Encoding
Enumerator	ok
Enumerator::Lazy	ok
Method	TypeError: allocator undefined for Method
UnboundMethod	TypeError: allocator undefined for UnboundMethod
Proc	TypeError: allocator undefined for Proc
Binding	TypeError: allocator undefined for Binding
Module	NoMethodError: undefined method 'allocate' for class Module
Class	ok
Thread	TypeError: allocator undefined for Thread
Mutex	ok
Queue	ok
SizedQueue	ok
ConditionVariable	ok
ThreadGroup	ok
Fiber	ok
Random	ok
ObjectSpace::WeakMap	ok
Digest::MD5	ok
Zlib::Deflate	ok
Zlib::Inflate	ok
Socket	ok
TCPSocket	ok
Etc::Passwd	ok
Process::Status	ok
Exception	ok
RuntimeError	ok
Errno::ENOENT	ok
TracePoint	TypeError: allocator undefined for TracePoint
Symbol	TypeError: allocator undefined for Symbol
Integer	TypeError: allocator undefined for Integer
Float	TypeError: allocator undefined for Float
subclass	NoMethodError
respond_to?	[false]
classes	[File, IO, Socket, TCPSocket, Date, DateTime, Thread::Queue, Thread::SizedQueue]
