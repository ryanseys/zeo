# The ffi gem's Ruby-side surface. The native half comes first (CRuby's
# loader idiom), so the FFI module and its native classes exist to be
# extended below.
require "ffi.so"

module FFI
  # The gem's exception hierarchy. Defined here rather than natively because
  # a feature-gated native class can't register a constructible exception
  # (see `crates/zeo-rt/src/ext/mod.rs`); as ordinary user classes they are
  # raisable by name from the native half (`FFI::NullPointerError` guards
  # every NULL read/write).
  class Error < StandardError; end
  class NullPointerError < Error; end
  class NotFoundError < Error; end

  module Platform
    def self.windows?
      false
    end

    def self.mac?
      RUBY_PLATFORM.include?("darwin")
    end

    def self.unix?
      true
    end
  end

  # A library module `extend`s this and speaks in DIRECTIVES the compiler
  # resolves at lowering time. The module exists at runtime because a
  # `def self.extended(host)` hook runs `host.extend FFI::Library` (and then
  # `host.typedef ...`) for real when the host's class body executes --
  # chef's Win32 API modules all share their FFI setup that way. By then
  # every attach site is already compiled, so the directives the hooks call
  # are no-ops.
  module Library
    def typedef(*) end

    def ffi_convention(*) end
  end

  # The custom-parameter-type protocol. zeo's marshaling converts through
  # `#to_ptr` rather than these hooks, so the module only records the declared
  # native type; classes extending it (fiddle's Pointer) override
  # `to_native`/`from_native` themselves.
  module DataConverter
    def native_type(type = nil)
      @native_type = type unless type.nil?
      @native_type
    end

    def to_native(value, _ctx)
      value
    end

    def from_native(value, _ctx)
      value
    end
  end
end
