# The reduce accumulator holds what the method answers, through both shapes.
# (spinel issue #3240)
def reducer(state, action)
  case action
  in { type: :inc, by: }
    state.merge(count: state[:count] + by)
  else
    state
  end
end
actions = [{ type: :inc, by: 5 }]
final = actions.reduce({ count: 0 }) { |s, a| reducer(s, a) }
puts final[:count]
history = actions.each_with_object([{ count: 0 }]) do |action, acc|
  acc << reducer(acc.last, action)
end
puts history.length
__END__
5
2
