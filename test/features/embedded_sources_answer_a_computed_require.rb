# `--embed-sources`: a computed `require` the compiler cannot resolve finds
# the file inside the PROGRAM, with no filesystem involved.
#@ zeo: --embed-sources features/embedded_sources_answer_a_computed_require/lib

$LOAD_PATH.unshift(File.expand_path("embedded_sources_answer_a_computed_require/lib", __dir__))
require_relative "embedded_sources_answer_a_computed_require/prog"
__END__
#@ stderr
features/embedded_sources_answer_a_computed_require.rb:3:in '<main>': uninitialized constant Greeter (NameError)
#@ exit 1
