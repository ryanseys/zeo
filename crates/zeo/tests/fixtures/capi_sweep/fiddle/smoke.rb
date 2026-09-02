require "fiddle"

libc = Fiddle.dlopen(nil)
strlen = Fiddle::Function.new(libc["strlen"], [Fiddle::TYPE_VOIDP], Fiddle::TYPE_SIZE_T)
puts strlen.call("hello")
puts Fiddle::SIZEOF_LONG, Fiddle::SIZEOF_VOIDP, Fiddle::ALIGN_DOUBLE
ptr = Fiddle::Pointer.malloc(16, Fiddle::RUBY_FREE)
ptr[0, 4] = "abc\0"
puts ptr.to_s, ptr.size
abs = Fiddle::Function.new(libc["abs"], [Fiddle::TYPE_INT], Fiddle::TYPE_INT)
puts abs.call(-42)
