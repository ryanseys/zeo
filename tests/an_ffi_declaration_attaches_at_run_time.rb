# zeo has two tiers for an `FFI::Library` declaration, and which one runs is
# decided by whether the COMPILER could see it.
#
# A declaration in a file the compiler lowered is consumed there: the whole
# C signature rides in `.rodata` and the module's `ffi_lib`/`attach_function`
# rows never run. A declaration the compiler could not see -- one a run-time
# `eval` compiled -- runs those rows, which attach for real over the gem's
# own runtime objects (`DynamicLibrary`, `Function`, `VariadicInvoker`).
#
# The first section pins the part BOTH tiers share: `extend FFI::Library` is
# a real `extend`, so the module is one.
require "ffi"

module Compiled
  extend FFI::Library
  ffi_lib FFI::Library::LIBC
  attach_function :abs, [:int], :int
end
p Compiled.abs(-7)
p Compiled.is_a?(FFI::Library)
p Compiled.singleton_class.ancestors.map(&:to_s).include?("FFI::Library")

def show
  yield
rescue Exception => e
  puts "#{e.class}: #{e.message.lines.first.to_s.strip[0, 40]}"
end

# A whole declaration the compiler never saw, in every argument shape: the
# 3-argument form, the 4-argument form that renames the C symbol, and a
# `typedef` the same body declared.
eval <<~SRC
  module Attached
    extend FFI::Library
    ffi_lib FFI::Library::LIBC
    typedef :int, :my_int
    attach_function :abs, [:my_int], :my_int
    attach_function :my_labs, :labs, [:long], :long
    attach_function :strlen, [:string], :ulong
  end
SRC
show { p Attached.abs(-7) }
show { p Attached.my_labs(-9) }
show { p Attached.strlen("hello") }
show { p (Attached.singleton_methods & %i[abs my_labs strlen]).sort }
show { p Attached.instance_methods(false).sort }

# A bare library name goes through the gem's own `LibraryPath` mangling.
show do
  eval <<~SRC
    module Mathy
      extend FFI::Library
      ffi_lib "m"
      attach_function :zeo_cos, :cos, [:double], :double
    end
  SRC
  p Mathy.zeo_cos(0.0)
end

# A variadic signature attaches as the gem's `VariadicInvoker`, so the
# trailing arguments arrive as (type, value) pairs.
show do
  eval <<~SRC
    module Va
      extend FFI::Library
      ffi_lib FFI::Library::LIBC
      attach_function :snprintf, [:pointer, :ulong, :string, :varargs], :int
    end
  SRC
  buf = FFI::MemoryPointer.new(:char, 32)
  p Va.snprintf(buf, 32, "%d-%s", :int, 42, :string, "hi")
  p buf.read_string
end

# `attach_function` answers the callable it installed.
show do
  m = Module.new
  m.extend FFI::Library
  m.ffi_lib FFI::Library::LIBC
  p m.attach_function(:abs, [:int], :int).class.to_s
end

# The three ways a declaration fails, each with the gem's own class.
show do
  eval <<~SRC
    module Missing
      extend FFI::Library
      ffi_lib FFI::Library::LIBC
      attach_function :zzz_not_a_symbol, [:int], :int
    end
  SRC
end
show do
  eval <<~SRC
    module NoSuch
      extend FFI::Library
      ffi_lib "definitely_not_a_library_zzz"
    end
  SRC
end
show do
  eval <<~SRC
    module NoLib
      extend FFI::Library
      attach_function :abs, [:int], :int
    end
  SRC
end
