# An FFI directive under a class-body `if`/`unless` whose predicate is fixed
# for the build target (`FFI::Platform::ADDRESS_SIZE`, a `RUBY_PLATFORM`
# regexp, `Gem.win_platform?`) folds at definition time and still reaches the
# FFI dispatch -- vips declares its `:gtype` typedef exactly this way. The
# windows-only branches must DROP: their declarations never register, and
# the module answers `respond_to?` accordingly, both oracle-verified.
require "ffi"

module M
  extend FFI::Library
  ffi_lib "m"
  if FFI::Platform::ADDRESS_SIZE == 64
    typedef :uint64, :gtype
  else
    typedef :uint32, :gtype
  end
  unless RUBY_PLATFORM =~ /mswin|mingw|windows/
    attach_function :my_fabs, :fabs, [:double], :double
  end
  if RUBY_PLATFORM =~ /mswin|mingw/
    attach_function :nope, :DoesNotExistAnywhere, [], :void
  end
  if Gem.win_platform?
    attach_function :also_nope, :AlsoAbsent, [], :void
  end
  if RUBY_PLATFORM.include?("darwin") || RUBY_PLATFORM.include?("linux")
    attach_function :my_floor, :floor, [:double], :double
  end

  # The gem scopes a library's `typedef` to that library: a struct must nest
  # INSIDE the module to name `:gtype` (a top-level one raises TypeError).
  class S < FFI::Struct
    layout :g, :gtype
  end
end

puts M::S.size
puts M.my_fabs(-2.5)
puts M.my_floor(3.75)
puts M.respond_to?(:nope)
puts M.respond_to?(:also_nope)
__END__
8
2.5
3.0
false
false
