# Issues #724, #726. Tuples (fixed-arity return types from
# Integer#divmod, Array#minmax, String#partition, etc.) used to fall
# through to the unresolved-call warning when passed to inspect.
# Now any tuple type goes through the generic tuple-inspect builder.

puts [1, 2, 3].minmax.inspect              # tuple:int,int
puts 17.divmod(5).inspect                   # tuple:int,int
puts "hello world".partition(" ").inspect  # tuple:string,string,string
puts "abc".rpartition("b").inspect         # tuple:string,string,string
