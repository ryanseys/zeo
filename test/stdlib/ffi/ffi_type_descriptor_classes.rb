# `FFI::Type`'s four CLASS constants -- the non-scalar type descriptors.
# `Array`/`Function`/`Struct` alias the canonical `FFI::ArrayType`/
# `FFI::FunctionType`/`FFI::StructByValue`; `Mapped` is named under `Type`
# and subclasses it; `CallbackInfo` is the third name for `FunctionType`.
require "ffi"

p [
  FFI::Type::Array == FFI::ArrayType,
  FFI::Type::Function == FFI::FunctionType,
  FFI::Type::Struct == FFI::StructByValue,
  FFI::CallbackInfo == FFI::FunctionType,
]
p [FFI::Type::Array.name, FFI::Type::Function.name, FFI::Type::Struct.name, FFI::Type::Mapped.name]
p FFI::Type::Array.superclass == FFI::Type
p FFI::Type::Mapped.ancestors[0, 3]
__END__
[true, true, true, true]
["FFI::ArrayType", "FFI::FunctionType", "FFI::StructByValue", "FFI::Type::Mapped"]
true
[FFI::Type::Mapped, FFI::Type, Object]
