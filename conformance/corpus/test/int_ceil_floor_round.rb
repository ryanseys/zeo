# Integer#ceil / floor / round / truncate without a precision arg
# return self per CRuby. Previously fell through to the unresolved-
# call path and emitted 0.
puts 3.ceil
puts 3.floor
puts 3.round
puts 3.truncate
puts (-3).ceil
puts (-3).floor
puts (-3).round
puts (-3).truncate
