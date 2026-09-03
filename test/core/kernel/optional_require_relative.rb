# A `require_relative` under a `rescue LoadError` is the optional-native-half
# idiom: rgeo requires its C implementation and catches the LoadError when it
# is absent. The missing target keeps its CALL and raises at runtime for the
# rescue to catch, instead of failing the compile.
begin
  require_relative "no_such_native_half"
rescue LoadError => e
  puts "fallback: #{e.class}"
end

# The protection is the begin BODY only: a bare rescue catches StandardError,
# which does not cover LoadError, so this shape stays a compile error and is
# not in this file. A resolvable require_relative under the same rescue still
# splices; nothing here tests it twice.
puts "continues"
__END__
fallback: LoadError
continues
