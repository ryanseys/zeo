# A `require` of a computed name loads a file at run time. That file
# requires a library the program has already loaded, so its `require`
# answers false and the library does not run a second time.
require "forwardable"
$LOAD_PATH.unshift(File.expand_path("a_run_time_compile_skips_a_loaded_feature", __dir__))
name = "wants_forwardable"
p require(name)
p WantsForwardable.ok
p $forwardable_again
p require(name)
__END__
true
true
false
false
