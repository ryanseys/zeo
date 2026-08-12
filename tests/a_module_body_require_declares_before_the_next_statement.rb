# A require written in a `class`/`module` BODY runs mid-body in CRuby, so
# the target's declarations exist before the statements below it -- libuv
# requires its `ext/types` (every typedef and enum) three lines above the
# `attach_function`s that spend them. The vocabulary must lower first; the
# required file's statements still execute in the trailing position the
# splice always gave them.
require "ffi"

module Loop
  extend FFI::Library
  ffi_lib FFI::Library::LIBC

  require_relative "a_module_body_require_declares_before_the_next_statement/uv_types"

  attach_function :big_abs, :labs, [:uv_ret], :uv_ret
  # The enum declared by the sibling is vocabulary here too.
  attach_function :kind_abs, :abs, [:uv_kind], :int
end

p Loop.big_abs(-3_000_000_000)
p Loop.kind_abs(:udp)
puts "still running"
