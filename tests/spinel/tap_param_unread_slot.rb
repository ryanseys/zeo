r = {1 => []}.each_with_index.map do |(k, v), _i|
  v.tap { |_| }
end
p r

pairs = {"a" => 1, "b" => 2}.each_with_index.map do |(k, v), i|
  k.tap { |unused| }
  [k, v, i]
end
p pairs

n = {1 => 2}.each_with_index.map do |(k, v), _i|
  v.then { |ignored| k }
end
p n

acc = [].tap { |a| a << 1 }
p acc
