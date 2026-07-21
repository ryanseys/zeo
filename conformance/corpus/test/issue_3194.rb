require "ostruct"
hash = {a: 1}
object = OpenStruct.new(hash)
raise unless object[:a] == 1
puts "ok3194"
