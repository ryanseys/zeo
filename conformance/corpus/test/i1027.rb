# Issue #1027: string interpolation of a method that returns sp_String *
# (the mutable-string type backing `String.new` + `<<`) emitted the
# pointer cast to long long with `%lld` instead of reading the string
# contents with `%s` + sp_String_cstr.
#
# The dispatch chain in compile_string_interp had arms for "int",
# "bigint", "float", "string", "bool", "poly", "class", "encoding",
# "symbol", "exception", "nil", and the array families — but no arm
# for "mutable_str", so an interpolated sp_String * fell through to
# the integer fallback and rendered as a pointer address.
#
# Direct `<<` on the same value already worked via sp_String_append
# (the type info exists; only the interp format-spec dispatch missed
# the arm). Surfaced in roundhouse view emit after a lowering coalesced
# `io << x; io << y; io << z` runs into `io << "...#{y}..."`-style
# string-interpolation. Other roundhouse targets (ts/crystal/rust/go/
# ruby) interpolate cleanly because their string primitives accept
# the mutable variant directly; only spinel needed the missing arm.

module M
  def self.make
    s = String.new
    s << "hello"
    s
  end
end

puts "[#{M.make}]"
