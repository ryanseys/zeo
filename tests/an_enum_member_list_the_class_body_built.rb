# An `enum` member list is written three ways that are all compile-time
# values: a word array mapped to symbols (`%w(...).map(&:to_sym)`, which is
# what spotify does four times), a body-local array the class body built up
# under guards, and that same array splatted in. All three name the members
# the C header names, so all three lower to the same conversion table.
#
# The members are observable through a function typed with the enum: ffi
# converts the symbol to its integer on the way in.
require "ffi"

module L
  extend FFI::Library
  ffi_lib FFI::Library::LIBC

  # `String#to_sym` is what the gem does to a string member anyway.
  enum :rate, %w(k160 k320 k96).map(&:to_sym)

  colours = [:red, :green]
  colours.push(:blue) if RUBY_PLATFORM =~ /darwin|linux|bsd/
  colours.push(:infra) if RUBY_PLATFORM =~ /mswin/
  enum :colour, colours

  attach_function :rate_code, :abs, [:rate], :int
  attach_function :colour_code, :abs, [:colour], :int
end

p L.rate_code(:k160)
p L.rate_code(:k320)
p L.rate_code(:k96)
p L.colour_code(:red)
p L.colour_code(:blue)
puts "still running"
