require "ffi"

module Curl
  extend FFI::Library
  ffi_lib FFI::Library::LIBC

  def self.status_codes = [:ok, :partial, :failed]

  # Declared ABOVE the require, and used BELOW it, in the required file.
  class Stamp < FFI::Struct
    layout :sec, :long
  end

  # The reopen that declares the vocabulary, required from INSIDE the body
  # that establishes what the module is.
  require_relative "vocabulary"

  attach_function :pick, :abs, [:status], :status
  attach_function :widen, :labs, [:ticks], :ticks
end
