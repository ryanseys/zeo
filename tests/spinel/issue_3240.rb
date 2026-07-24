# A method returning a hash, folded once via reduce (concrete seed) and once
# pushed via each_with_object (poly), widens to poly; the reduce accumulator
# must widen to match rather than assigning the boxed result into a concrete
# hash slot.
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
