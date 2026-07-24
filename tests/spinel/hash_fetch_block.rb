h_si = { a: 1, b: 2 }
puts h_si.fetch(:a) { 99 }
puts h_si.fetch(:missing) { 99 }

h_ss = { x: "one", y: "two" }
puts h_ss.fetch(:x) { "fallback" }
puts h_ss.fetch(:missing) { "fallback" }

h_sp = { a: 1, b: "two" }
puts h_sp.fetch(:b) { "fallback" }
puts h_sp.fetch(:missing) { "fallback" }

h_str_int = { "a" => 1, "b" => 2 }
puts h_str_int.fetch("a") { 99 }
puts h_str_int.fetch("missing") { 99 }

h_str_str = { "x" => "one", "y" => "two" }
puts h_str_str.fetch("x") { "fallback" }
puts h_str_str.fetch("missing") { "fallback" }

h_str_p = { "a" => 1, "b" => "two" }
puts h_str_p.fetch("b") { "fallback" }
puts h_str_p.fetch("missing") { "fallback" }

# block can be multi-statement
result = h_si.fetch(:missing) {
  x = 10
  y = 20
  x + y
}
puts result
