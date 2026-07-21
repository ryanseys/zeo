def render(v)
  return v.to_s unless v.is_a?(Hash)
  "{" + v.map { |k, val| "#{k}=#{render(val)}" }.join(",") + "}"
end
p render({ "a" => 1, "b" => { "c" => 2, "d" => 3 } })
p render({ 1 => 10, 2 => 20 })
h = { "x" => 1, "y" => 2 }
p h.map { |k, val| [k, val * 10] }
p h.map { |pair| pair.inspect }
