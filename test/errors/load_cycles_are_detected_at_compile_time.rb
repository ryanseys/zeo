# Real Ruby would recurse forever at runtime (load has no dedup); a
# compile-time resolver rejects the cycle loudly instead.

require_relative "load_cycles_are_detected_at_compile_time/main"
__END__
#@ stderr
zeo::lower

  × errors/load_cycles_are_detected_at_compile_time/main.rb: errors/load_cycles_are_detected_at_compile_time/selfload.rb: `load` cycle detected: errors/load_cycles_are_detected_at_compile_time/selfload.rb is already being loaded (real Ruby would recurse forever at runtime; this compiler rejects it at compile time instead)
  help: zeo can't compile this yet

#@ exit 1
