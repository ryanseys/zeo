# DECIDED DIVERGENCE, layout only. With a SMALL `line_width:`, psych
# switches a long scalar to the FOLDED style (`>-`) where zeo keeps the
# plain style and wraps it.
#
# Both are the same string: a folded scalar joins its lines with spaces,
# which is exactly what the wrapped plain scalar already spells. They load
# back identically -- the rows below say so -- and the default width
# (80) agrees byte for byte, which is what every real dump uses.
#
# Matching it means porting libyaml's style chooser, whose rule for
# preferring `>` over a wrapped plain scalar depends on the emitter's
# column bookkeeping rather than on the value.

require "yaml"

long = ("word " * 20).strip

dumped = YAML.dump({ "s" => long }, line_width: 20)
puts dumped
puts "lines\t#{dumped.lines.size > 2}"
puts "round trip\t#{YAML.unsafe_load(dumped) == { 's' => long }}"

# The DEFAULT width agrees exactly, which is the case that matters.
puts "default\t#{YAML.dump({ 's' => long }) == YAML.dump({ 's' => long }, line_width: 80)}"
__END__
---
s: word word word word
  word word word
  word word word
  word word word
  word word word
  word word word
  word
lines	true
round trip	true
default	true
