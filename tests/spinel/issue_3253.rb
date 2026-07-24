# each_with_object with a Hash memo the block never reads must still compile.
[1, 2].each_with_object({}) { |x, acc| p x }
{ a: 1 }.each_with_object({}) { |(k, v), h| p k }
{ a: 1, b: 2 }.each_with_object([]) { |(k, v), acc| p v }
