# A require-gated builtin's CONSTANT does not exist until its `require` runs.
# zeo registers the class for the whole program (a feature no file requires
# registers nothing at all), and `HirNode::FeatureLoaded` reveals it at the
# require's own line -- so `defined?` and a bare reference ask instead of
# folding, exactly as they already did for a class defined under a guard zeo
# cannot decide.

p Object.const_defined?(:Base64)
p defined?(Base64)
begin
  Base64
rescue NameError => e
  puts e.message
end

require "base64"

p Object.const_defined?(:Base64)
p defined?(Base64)
p Base64.encode64("hi")

# A method written above the require reads the constant when it RUNS.
def probe = defined?(Zlib) ? "yes" : "no"
p probe
require "zlib"
p probe
p Zlib::Deflate.is_a?(Class)

# A feature ruby loads before line 1 needs no require at all.
p defined?(Monitor)
p defined?(Set)
__END__
false
nil
uninitialized constant Base64
true
"constant"
"aGk=\n"
"no"
"yes"
true
"constant"
"constant"
