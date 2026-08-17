# A method taking `**kwrest` takes every keyword the call passed, so none of
# them is a positional hash argument: the keywords-only call used to bind the
# hash to the optional positional as well, and `event.nil?` answered false
# (#3808).
def inner(event = nil, **filters)
  puts "nil?=#{event.nil?} keys=#{filters.keys.inspect}"
end
inner(key_down: :escape)
inner(:sym, key_down: :escape)
inner(nil, key_down: :escape)
def on(event = nil, **filters)
  if event.nil?
    "keywords"
  else
    "positional #{event.inspect}"
  end
end
puts on(key_down: :escape)
puts on(:quit)
