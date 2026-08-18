# A class method calling bare `new` inside a class whose constant is
# CONCEALED -- `FFI.constants` is a namespace observer, so every member of it
# becomes a runtime question and `.new` reads the constant through the
# concealment check. That route answers a plain value, and the implicit-self
# call site has to agree with it: boxing what it already boxed is invalid Rust,
# not a wrong answer, so this test is a compile check as much as a value one.
require "ffi"

p FFI.constants.grep(/^TYPE_/).sort.first(3)
p FFI::LibraryPath.wrap("m").class
p FFI::LibraryPath.wrap("m").name
p FFI::LibraryPath.wrap("/usr/lib/libm.dylib")
p FFI::LibraryPath.wrap(FFI::LibraryPath.new("z")).name
