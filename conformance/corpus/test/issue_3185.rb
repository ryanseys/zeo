def describe7(event)
  case event
  in { type: :move, dx:, dy: } if dx == 0 && dy == 0 then "no movement"
  in { type: :move, dx:, dy: } then "move by #{dx},#{dy}"
  in { type: :stop } | { type: :halt } then "stopping"
  else "unknown"
  end
end
puts describe7({ type: :move, dx: 0, dy: 0 })
puts describe7({ type: :halt })
