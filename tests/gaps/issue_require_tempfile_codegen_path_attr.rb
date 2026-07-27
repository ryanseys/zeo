# A bare `require "tempfile"` fails to compile: the generated Rust emits a
# bare `path` identifier where Tempfile's `path` local/method should be
# referenced, and rustc parses it as the built-in `#[path]` attribute
# instead of a value ("expected value, found built-in attribute `path`").
require "tempfile"
p Tempfile.respond_to?(:new)
