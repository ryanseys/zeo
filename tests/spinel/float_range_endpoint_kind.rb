# A range is a Float range when EITHER endpoint is one, and its endpoint
# methods answer the endpoint the caller wrote (#3837).
p((-Float::INFINITY..5).max)
p((-Float::INFINITY..5).last)
p((1..5.0).min)
p((1.0..5.0).max)
p((1..5).max)
p((0.5..5).max)
p((1..5.5).min)
