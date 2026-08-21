# A `then` block whose body always `break`s completes normally nowhere, so it
# published no result type and the value slot the block substrate writes was
# declared `void`, which cannot declare a C variable. The break delivers its
# value through the break stack, so the slot is dead on that path -- it just
# has to have a type. A body that CAN fall through was always fine, which is
# what the conditional-break lines below hold in place.
p 1.then { next 2 }
p 1.tap { next 2 }
p 1.tap { break 2 }
p [1].each { break 2 }
p 1.then { |x| break 2 if x == 1; 3 }
p 1.then { |x| break 2 if x == 9; 3 }
p "s".then { break }
p [1,2].map { |x| x.then { break 9 } }
def m
  r = 1.then { break 5 }
  r
end
p m
h = {"k" => 1}
p h.then { break [] }
p 2.then { break 3.5 }
