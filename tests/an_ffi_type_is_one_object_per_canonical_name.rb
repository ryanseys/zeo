# `FFI::Type::Builtin` holds one instance per CANONICAL type, and the aliases
# are extra constants naming the SAME object -- `:char` really is
# `Type::Builtin::INT8`, which is why `CHAR.inspect` says `INT8`.
#
# The canonical set is not the set of distinct C widths. `LONG` and `INT64`
# are separate objects that compare UNEQUAL although both are eight signed
# bytes on this target, and so are `ULONG` and `UINT64`. Collapsing them by
# width made `ffi_args.last == FFI::Type::Builtin::LONG` answer true for an
# `:int64` argument, where the gem answers false.
#
# Only the constants naming a TYPE are listed. `FFI::Type` also carries four
# naming a CLASS -- `Array`, `Function`, `Struct` and `Mapped`, for the
# non-scalar type descriptors -- which zeo does not have yet.
require "ffi"

types = FFI::Type::Builtin.constants.sort.select do |name|
  FFI::Type::Builtin.const_get(name).is_a?(FFI::Type)
end
p types
types.each { |name| puts "#{name}\t#{FFI::Type::Builtin.const_get(name).inspect}" }

# Two names for one type ARE one object; two types of one width are not.
p [
  FFI::Type::Builtin::CHAR.equal?(FFI::Type::Builtin::INT8),
  FFI::Type::Builtin::DOUBLE.equal?(FFI::Type::Builtin::FLOAT64),
  FFI::Type::Builtin::LONG_LONG.equal?(FFI::Type::Builtin::INT64),
  FFI::Type::Builtin::LONG.equal?(FFI::Type::Builtin::INT64),
  FFI::Type::Builtin::ULONG.equal?(FFI::Type::Builtin::UINT64),
]
p [
  FFI::Type::Builtin::CHAR == FFI::Type::Builtin::INT8,
  FFI::Type::Builtin::LONG == FFI::Type::Builtin::INT64,
  FFI::Type::Builtin::ULONG == FFI::Type::Builtin::UINT64,
  FFI::Type::Builtin::LONG.size == FFI::Type::Builtin::INT64.size,
]

# `FFI::Type` spells the same constants, and `VOID` there is the very object
# `Builtin::VOID` is.
p FFI::Type::VOID.equal?(FFI::Type::Builtin::VOID)
p [FFI::Type.instance_methods(false).sort, FFI::Type::Builtin.instance_methods(false).sort]

# `NativeType` and the top-level `TYPE_*` constants are a third and fourth
# name for the canonical set -- only the canonical names, no aliases.
p FFI::NativeType.constants.sort
p FFI.constants.grep(/^TYPE_/).sort
p [
  FFI::TYPE_INT32.equal?(FFI::Type::Builtin::INT32),
  FFI::NativeType::LONG.equal?(FFI::Type::Builtin::LONG),
]

# The keyword each spelling resolves to, and the lookup that reads it.
p FFI::TypeDefs.keys.first(29)
p %i[void int uint long ulong long_long float double pointer string buffer_in varargs]
  .map { |k| FFI.find_type(k).inspect }
p %i[char short int long float double pointer].map { |k| FFI.type_size(k) }
p FFI.find_type(FFI::Type::Builtin::BOOL).equal?(FFI::Type::Builtin::BOOL)
p FFI.find_type(:int, { int: FFI::Type::Builtin::INT8 }).inspect

# A runtime `typedef` is reflection: it answers `find_type` and leaves the
# published `TypeDefs` table alone, exactly as the gem's C half does.
FFI.typedef(:uint32, :my_handle_t)
p [FFI.find_type(:my_handle_t).inspect, FFI::TypeDefs[:my_handle_t]]

begin
  FFI.find_type(:definitely_not_a_type)
rescue TypeError => e
  p [e.class, e.message]
end
