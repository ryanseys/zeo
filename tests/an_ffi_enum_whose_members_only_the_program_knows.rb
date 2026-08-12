# An `enum` whose MEMBER LIST is not a literal. ethon writes
# `EasyCode = enum(:easy_code, easy_codes)`, where `easy_codes` is a method on
# a module it `extend`ed and returns eighty-odd symbols; gir_ffi reads its flag
# sets out of a shared library at load time. zeo used to refuse both.
#
# The ABI never depended on the members: an enum is a C `int` either way, so
# the extern signature is settled at compile time and only the symbol<->integer
# marshaling has to wait. It waits in a slot -- the class body fills it when it
# executes, exactly like a deferred `ffi_lib` -- and every signature lowered
# under the declaration reads it back.
#
# The gem decides the named-vs-anonymous shape by RUNTIME CLASS: `Library#enum`
# reads `:tag, [members]` when the second argument is an Array, and treats
# every argument as a member otherwise. `enum(:status, status_codes)` is
# therefore NAMED, while `enum(:level, *levels)` is not -- the splat puts
# members, not an Array, in `args[1]`.
require "ffi"

module Codes
  # An explicit Integer sets that member's value AND the counter, so `:weird`
  # is 18. Same rule as the literal tier, applied to values only the running
  # program has.
  def status_codes = [:ok, :partial, :failed, 17, :weird]
end

module Lib
  extend FFI::Library
  extend Codes
  # A candidate list forces the dlopen tier too, so both marshaling paths --
  # the compile-time extern and the libffi one -- run against the same slot.
  ffi_lib ["/no/such/dir/libnothing.so", FFI::Library::LIBC]

  Status = enum(:status, status_codes)

  class Record < FFI::Struct
    layout :code, :status, :n, :int
  end

  attach_function :pick, :abs, [:status], :status
end

p Lib.pick(:failed)
p Lib.pick(:weird)
p Lib.pick(:ok)
# An unnamed value passes through as an Integer, both ways.
p Lib.pick(99)

# A struct field of that enum reads and writes through the same slot.
r = Lib::Record.new
r[:code] = :failed
p r[:code]
r[:code] = :ok
p r[:code]
p Lib::Record.size
