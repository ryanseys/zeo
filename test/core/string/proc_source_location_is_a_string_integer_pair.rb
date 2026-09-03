loc = ->(x) { x }.source_location
p loc.class
p loc.length
p loc[0].class
p loc[1].class
__END__
Array
2
String
Integer
