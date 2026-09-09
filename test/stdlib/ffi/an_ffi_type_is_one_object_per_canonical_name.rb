#@ only: macos
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
# non-scalar type descriptors -- pinned by
# `test/stdlib/ffi/ffi_type_descriptor_classes.rb`.
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
__END__
[:BOOL, :BUFFER_IN, :BUFFER_INOUT, :BUFFER_OUT, :CHAR, :DOUBLE, :FLOAT, :FLOAT32, :FLOAT64, :INT, :INT16, :INT32, :INT64, :INT8, :LONG, :LONGDOUBLE, :LONG_LONG, :POINTER, :SCHAR, :SHORT, :SINT, :SLONG, :SLONG_LONG, :SSHORT, :STRING, :UCHAR, :UINT, :UINT16, :UINT32, :UINT64, :UINT8, :ULONG, :ULONG_LONG, :USHORT, :VARARGS, :VOID]
BOOL	#<FFI::Type::Builtin::BOOL size=1 alignment=1>
BUFFER_IN	#<FFI::Type::Builtin::BUFFER_IN size=8 alignment=8>
BUFFER_INOUT	#<FFI::Type::Builtin::BUFFER_INOUT size=8 alignment=8>
BUFFER_OUT	#<FFI::Type::Builtin::BUFFER_OUT size=8 alignment=8>
CHAR	#<FFI::Type::Builtin::INT8 size=1 alignment=1>
DOUBLE	#<FFI::Type::Builtin::FLOAT64 size=8 alignment=8>
FLOAT	#<FFI::Type::Builtin::FLOAT32 size=4 alignment=4>
FLOAT32	#<FFI::Type::Builtin::FLOAT32 size=4 alignment=4>
FLOAT64	#<FFI::Type::Builtin::FLOAT64 size=8 alignment=8>
INT	#<FFI::Type::Builtin::INT32 size=4 alignment=4>
INT16	#<FFI::Type::Builtin::INT16 size=2 alignment=2>
INT32	#<FFI::Type::Builtin::INT32 size=4 alignment=4>
INT64	#<FFI::Type::Builtin::INT64 size=8 alignment=8>
INT8	#<FFI::Type::Builtin::INT8 size=1 alignment=1>
LONG	#<FFI::Type::Builtin::LONG size=8 alignment=8>
LONGDOUBLE	#<FFI::Type::Builtin::LONGDOUBLE size=8 alignment=8>
LONG_LONG	#<FFI::Type::Builtin::INT64 size=8 alignment=8>
POINTER	#<FFI::Type::Builtin::POINTER size=8 alignment=8>
SCHAR	#<FFI::Type::Builtin::INT8 size=1 alignment=1>
SHORT	#<FFI::Type::Builtin::INT16 size=2 alignment=2>
SINT	#<FFI::Type::Builtin::INT32 size=4 alignment=4>
SLONG	#<FFI::Type::Builtin::LONG size=8 alignment=8>
SLONG_LONG	#<FFI::Type::Builtin::INT64 size=8 alignment=8>
SSHORT	#<FFI::Type::Builtin::INT16 size=2 alignment=2>
STRING	#<FFI::Type::Builtin::STRING size=8 alignment=8>
UCHAR	#<FFI::Type::Builtin::UINT8 size=1 alignment=1>
UINT	#<FFI::Type::Builtin::UINT32 size=4 alignment=4>
UINT16	#<FFI::Type::Builtin::UINT16 size=2 alignment=2>
UINT32	#<FFI::Type::Builtin::UINT32 size=4 alignment=4>
UINT64	#<FFI::Type::Builtin::UINT64 size=8 alignment=8>
UINT8	#<FFI::Type::Builtin::UINT8 size=1 alignment=1>
ULONG	#<FFI::Type::Builtin::ULONG size=8 alignment=8>
ULONG_LONG	#<FFI::Type::Builtin::UINT64 size=8 alignment=8>
USHORT	#<FFI::Type::Builtin::UINT16 size=2 alignment=2>
VARARGS	#<FFI::Type::Builtin::VARARGS size=1 alignment=1>
VOID	#<FFI::Type::Builtin::VOID size=1 alignment=1>
[true, true, true, false, false]
[true, false, false, true]
true
[[:alignment, :inspect, :size], [:inspect]]
[:BOOL, :BUFFER_IN, :BUFFER_INOUT, :BUFFER_OUT, :FLOAT32, :FLOAT64, :INT16, :INT32, :INT64, :INT8, :LONG, :LONGDOUBLE, :POINTER, :STRING, :UINT16, :UINT32, :UINT64, :UINT8, :ULONG, :VARARGS, :VOID]
[:TYPE_BOOL, :TYPE_BUFFER_IN, :TYPE_BUFFER_INOUT, :TYPE_BUFFER_OUT, :TYPE_FLOAT32, :TYPE_FLOAT64, :TYPE_INT16, :TYPE_INT32, :TYPE_INT64, :TYPE_INT8, :TYPE_LONG, :TYPE_LONGDOUBLE, :TYPE_POINTER, :TYPE_STRING, :TYPE_UINT16, :TYPE_UINT32, :TYPE_UINT64, :TYPE_UINT8, :TYPE_ULONG, :TYPE_VARARGS, :TYPE_VOID]
[true, true]
[:void, :bool, :string, :char, :uchar, :short, :ushort, :int, :uint, :long, :ulong, :long_long, :ulong_long, :float, :double, :long_double, :pointer, :int8, :uint8, :int16, :uint16, :int32, :uint32, :int64, :uint64, :buffer_in, :buffer_out, :buffer_inout, :varargs]
["#<FFI::Type::Builtin::VOID size=1 alignment=1>", "#<FFI::Type::Builtin::INT32 size=4 alignment=4>", "#<FFI::Type::Builtin::UINT32 size=4 alignment=4>", "#<FFI::Type::Builtin::LONG size=8 alignment=8>", "#<FFI::Type::Builtin::ULONG size=8 alignment=8>", "#<FFI::Type::Builtin::INT64 size=8 alignment=8>", "#<FFI::Type::Builtin::FLOAT32 size=4 alignment=4>", "#<FFI::Type::Builtin::FLOAT64 size=8 alignment=8>", "#<FFI::Type::Builtin::POINTER size=8 alignment=8>", "#<FFI::Type::Builtin::STRING size=8 alignment=8>", "#<FFI::Type::Builtin::BUFFER_IN size=8 alignment=8>", "#<FFI::Type::Builtin::VARARGS size=1 alignment=1>"]
[1, 2, 4, 8, 4, 8, 8]
true
"#<FFI::Type::Builtin::INT8 size=1 alignment=1>"
["#<FFI::Type::Builtin::UINT32 size=4 alignment=4>", nil]
[TypeError, "unable to resolve type 'definitely_not_a_type'"]
