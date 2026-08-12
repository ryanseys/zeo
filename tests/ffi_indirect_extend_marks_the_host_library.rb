# A module whose `def self.extended(host)` hook runs `host.extend
# FFI::Library` makes every module that extends IT an FFI library one step
# removed -- chef's Win32 API modules share one FFI setup this way. The
# hook's `host.typedef` stream replays into the host before its own
# directives lower, and the `extend` itself still happens (the host keeps
# the extender's ordinary methods).
require "ffi"

module Api
  def self.extended(host)
    host.extend FFI::Library
    host.typedef :uint32, :dword
    host.typedef :uint64, :qword
  end

  def helper_marker
    :helped
  end
end

module Win
  extend Api
  ffi_lib "m"
  enum :status, [:ok, 1, :bad]
  typedef :dword, :dword_alias
  attach_function :my_ceil, :ceil, [:double], :double

  class Box < FFI::Struct
    layout :d, :dword_alias, :q, :qword, :s, :status
  end
end

puts Win.my_ceil(1.25)
puts Win::Box.offset_of(:q)
puts Win::Box.size
puts Win.respond_to?(:helper_marker)
puts Win.helper_marker
