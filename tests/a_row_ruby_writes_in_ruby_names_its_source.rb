# `#source_location` on a row CRuby writes in RUBY (`<internal:>` files).
# Zeo implements those rows in Rust, which answers `nil` -- a decided
# divergence, measured and kept (the vendored-Ruby alternative was 22.7x
# slower and 3.5x bigger). The `.divergence` sidecar records why and the fix
# shape if it is ever worth it; `.expected` records ZEO's answers.
%i[to_i to_f to_r to_c rationalize].each do |m|
  puts "NilClass##{m}: #{NilClass.instance_method(m).source_location.inspect}"
end
puts "Pathname#basename: #{Pathname.instance_method(:basename).source_location.inspect}"
puts "Method#inspect: #{NilClass.instance_method(:to_i).inspect}"
# The control: a row ruby writes in C answers nil in BOTH, and always has.
puts "String#upcase: #{String.instance_method(:upcase).source_location.inspect}"
