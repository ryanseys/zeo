# `e` (a rescue clause's exception binding) is never narrowed to a
# concrete class (see `clif::control::lower_begin`) -- exercises
# `respond_to?`'s runtime `class_id()` fallback path, not the
# statically-known-class one the test above exercises.

begin
  raise "boom"
rescue => e
  puts e.respond_to?(:message)
  puts e.respond_to?(:not_a_thing)
end
__END__
true
false
