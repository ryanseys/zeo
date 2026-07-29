# fiddle over zeo's FFI: the gem's own pure-Ruby FFI backend
# (lib/fiddle/ffi_backend.rb, vendored from fiddle 1.1.8) runs over zeo's
# native ffi runtime tier instead of the fiddle C extension. The Importer
# DSL (fiddle/import, fiddle/struct) is not included -- it builds methods
# with module_eval on computed strings, which AOT compilation can't express.
Gem::Specification.new do |s|
  s.name = "fiddle"
  s.version = "1.1.8"
  s.summary = "A libffi wrapper: dlopen, Fiddle::Function, closures."
  s.require_paths = ["lib"]
end
