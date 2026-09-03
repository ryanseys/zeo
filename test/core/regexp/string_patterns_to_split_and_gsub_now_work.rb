# SEMANTICS FLIP: String patterns to split/gsub used to be
# compile-time rejections ("pass a Regexp literal instead"); the String
# table rows now implement them for real, so the static regexp path falls
# through to dynamic dispatch instead. Oracle-verified.

puts "a,b".split(",").inspect
puts "a,b".gsub(",", ";")
__END__
["a", "b"]
a;b
