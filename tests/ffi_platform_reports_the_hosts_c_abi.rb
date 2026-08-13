# `FFI::Platform` answers the host's platform and C-ABI facts as CONSTANTS,
# not only as the four predicates zeo used to carry. A gem reads them
# directly -- crabstone picks `:size_t`'s width with
# `FFI::Platform::ADDRESS_SIZE == 32`, and smartcard its entry-point suffix
# from `FFI::Platform.windows?` -- so an absent constant is a NameError at
# the gem's first line.
#
# The values a compile-time guard folds and the values the program prints
# have to be the same numbers, which is the last group here.
#
# OSVERSION and CONF_DIR are deliberately absent: both carry the version of
# the OS the interpreter was BUILT on, so they differ between any two hosts.
require "ffi"

p FFI::VERSION

p FFI::Platform.constants.sort

sizes = %i[
  ADDRESS_SIZE ADDRESS_ALIGN LONG_SIZE LONG_ALIGN
  INT8_SIZE INT8_ALIGN INT16_SIZE INT16_ALIGN
  INT32_SIZE INT32_ALIGN INT64_SIZE INT64_ALIGN
  FLOAT_SIZE FLOAT_ALIGN DOUBLE_SIZE DOUBLE_ALIGN
  LITTLE_ENDIAN BIG_ENDIAN BYTE_ORDER
]
p sizes.map { |c| FFI::Platform.const_get(c) }

p [FFI::Platform::OS, FFI::Platform::ARCH, FFI::Platform::NAME]
p [FFI::Platform::LIBPREFIX, FFI::Platform::LIBSUFFIX, FFI::Platform::LIBC]
p FFI::Library::LIBC == FFI::Platform::LIBC

# One IS_ flag per OS, and the predicate that reads it. macOS counts as a
# BSD, so `bsd?` and `mac?` are true together.
p FFI::Platform.constants.grep(/^IS_/).sort.map { |c| [c, FFI::Platform.const_get(c)] }
p [
  FFI::Platform.mac? == FFI::Platform::IS_MAC,
  FFI::Platform.windows? == FFI::Platform::IS_WINDOWS,
  FFI::Platform.solaris? == FFI::Platform::IS_SOLARIS,
  FFI::Platform.bsd? == FFI::Platform::IS_BSD,
  FFI::Platform.unix? == !FFI::Platform::IS_WINDOWS,
]
p FFI::Platform.is_os(FFI::Platform::OS)

# The exception hierarchy is load-bearing: `rescue LoadError` around a
# `require` is what catches a missing library or a missing symbol.
p [
  FFI::NullPointerError.superclass,
  FFI::NotFoundError.superclass,
  FFI::PlatformError.superclass,
]
p defined?(FFI::Error)

# Library name mapping. A bare name gains the platform's prefix and suffix,
# a name that already carries one keeps it, a path passes straight through,
# and `c` is the standard C library rather than a file called `libc`.
p %w[jpeg z libz.dylib /usr/lib/libz.dylib c].map { |n| FFI.map_library_name(n) }
p FFI::LibraryPath.new("vips", abi_number: 42).to_s
p FFI::LibraryPath.new("vips", root: "/opt/lib").to_s
begin
  FFI.map_library_name(:crypto)
rescue TypeError => e
  p [e.class, e.message]
end

p [FFI.type_size(:int), FFI.type_size(:pointer), FFI.type_size(:double)]
p FFI.find_type(:ulong).equal?(FFI::Type::Builtin::ULONG)
begin
  FFI.find_type(:nope)
rescue TypeError => e
  p [e.class, e.message]
end

p FFI::CURRENT_PROCESS.equal?(FFI::USE_THIS_PROCESS_AS_LIBRARY)
p FFI.make_shareable([1, 2]).frozen?

# A guard the compiler folds and the constant the program reads answer the
# same question -- so a `typedef` chosen at compile time cannot contradict
# what the running program says the width is.
size_t = FFI::Platform::ADDRESS_SIZE == 32 ? :ulong : :ulong_long
p [size_t, FFI::Platform::ADDRESS_SIZE == 64, FFI::Platform::ARCH == "aarch64"]
