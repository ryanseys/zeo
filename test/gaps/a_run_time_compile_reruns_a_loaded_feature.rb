require "forwardable"
$LOAD_PATH.unshift(File.expand_path("fixtures/reloaded_feature", __dir__))
name = "wants_forwardable"
require name
p WantsForwardable.ok
__END__
true
