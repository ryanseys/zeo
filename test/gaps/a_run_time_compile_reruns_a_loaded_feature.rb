# A `require` of a computed name, for a feature that itself requires a
# library the program has already loaded. ruby runs the new feature once and
# skips the already-loaded one; zeo's run-time compile reaches the loaded
# feature again.
require "forwardable"
$LOAD_PATH.unshift(File.expand_path("fixtures/reloaded_feature", __dir__))
name = "wants_forwardable"
require name
p WantsForwardable.ok
__END__
true
