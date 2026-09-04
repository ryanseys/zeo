puts "stops: start"
STOPS_MARK = :set
return if STOPS_MARK

puts "stops: never"
require_relative "never_required"
