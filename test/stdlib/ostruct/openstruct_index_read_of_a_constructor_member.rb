# `object[:a]` answers what the hash held.
require "ostruct"
hash = {a: 1}
object = OpenStruct.new(hash)
raise unless object[:a] == 1
puts "ok3194"
__END__
ok3194
