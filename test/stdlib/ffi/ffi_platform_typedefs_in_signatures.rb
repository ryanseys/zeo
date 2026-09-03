# A POSIX typedef whose width differs between zeo's targets (mode_t is u16
# on macOS, u32 on glibc) is legal in argument/return position -- the
# generated code spells the TARGET's own libc alias, so rustc supplies the
# real width.
#
# A struct FIELD cannot wait that long: its width fixes every following
# field's offset while the layout lowers. zeo answers from the libc the
# COMPILER was built against, which is exact for the one target it emits for
# -- the same assumption the baked RUBY_PLATFORM and RbConfig::CONFIG already
# make. The struct checks below print only facts that hold on every target,
# since the widths themselves deliberately do not.
require "ffi"
require "tempfile"

module C
  extend FFI::Library
  ffi_lib FFI::Library::LIBC
  attach_function :chmod, [:string, :mode_t], :int
  attach_function :clock, [], :clock_t
end

f = Tempfile.new("zeo-mode")
puts C.chmod(f.path, 0o644)
puts format("%o", File.stat(f.path).mode & 0o7777)
puts C.chmod(f.path, 0o600)
puts format("%o", File.stat(f.path).mode & 0o7777)
puts C.clock.is_a?(Integer)
f.close!

class Timing < FFI::Struct
  layout :marker, :uint32, :elapsed, :clock_t
end

class Marker < FFI::Struct
  layout :marker, :uint32
end

t = Timing.new
t[:marker] = 7
t[:elapsed] = 123_456
puts t[:marker]
puts t[:elapsed]
puts Timing.offset_of(:marker)
puts Timing.offset_of(:elapsed) >= 4
puts Timing.size > Marker.size

# `sa_family_t` is the narrowest of the family -- one byte on macOS, two on
# glibc -- so it goes LAST, where its width shifts nothing.
class Addr < FFI::Struct
  layout :data, [:char, 8], :family, :sa_family_t
end

a = Addr.new
a[:family] = 2
puts a[:family]
puts Addr.offset_of(:data)
puts Addr.offset_of(:family) >= 8
__END__
0
644
0
600
true
7
123456
0
true
true
2
0
true
