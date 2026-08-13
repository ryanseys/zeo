# A module whose `self.included(base)` hook `class_eval`s a `layout` into
# whoever includes it. gssapi writes every one of its buffer structs this way:
# `GssBufferDescLayout` carries the two fields AND the two readers, and the
# structs themselves have empty bodies apart from the `include`.
#
# zeo saw a struct class with no layout, so embedding one inline in another
# struct reported "its `layout` directive never lowered".
#
# The hook's block SOURCE is recorded when the module lowers (a prism node is
# neither `Clone` nor storable past its parse) and re-parsed at the include
# site, where its statements take the ordinary struct-body path -- so the
# layout synthesizes accessors and records offsets exactly as a written one
# does, and the `def`s beside it come along.
require "ffi"

module GSSAPI
  module LibGSSAPI
    extend FFI::Library
    ffi_lib FFI::Library::LIBC

    module BufferLayout
      def self.included(base)
        base.class_eval do
          layout :length, :uint32, :value, :pointer

          def len2 = self[:length] * 2
        end
      end
    end

    # The wrapper the real structs descend from -- the shape that made
    # `UnManagedGssBufferDesc` invisible as a struct in the first place.
    class GssUMStruct < FFI::Struct
      def tag = "um"
    end

    class UnManagedGssBufferDesc < GssUMStruct
      include BufferLayout
    end

    # Embedded BY VALUE, which is what needs the layout rather than just the
    # name.
    class GssIOVBufferDesc < FFI::Struct
      layout :type, :uint32, :buffer, UnManagedGssBufferDesc
    end
  end
end

b = GSSAPI::LibGSSAPI::UnManagedGssBufferDesc.new
b[:length] = 5
p b.len2
p b.tag
p GSSAPI::LibGSSAPI::UnManagedGssBufferDesc.size
p GSSAPI::LibGSSAPI::GssIOVBufferDesc.size
p GSSAPI::LibGSSAPI::GssIOVBufferDesc.offset_of(:buffer)
