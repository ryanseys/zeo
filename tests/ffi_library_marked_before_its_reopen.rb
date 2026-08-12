# A class-body `require` pre-LOWERS ahead of the requiring file's own
# statements, so a required file's declarations exist before the statements
# below the require -- libuv's `require 'libuv/ext/types'`, three lines above
# the `attach_function`s that spend its enums.
#
# The dependency runs both ways. ethon writes `extend ::FFI::Library` in
# `curl.rb` and then requires `curls/constants.rb`, which REOPENS that module
# thirteen lines lower to declare its enums. Lowering the required file first
# left the reopen looking like a plain namespace, so no directive in it was
# recognized at all and every signature naming one was an unsupported type.
#
# What each body IS -- an FFI library, a struct class -- is now read off the
# requiring file's own syntax before anything it requires lowers. Only the
# marks: what a body DECLARES needs the alias tables that real lowering has.
require_relative "ffi_library_marked_before_its_reopen/curl"

p Curl.pick(:failed)
p Curl.pick(:ok)
p Curl.widen(-7)
