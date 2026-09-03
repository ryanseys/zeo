# A large constant table lands in the binary's data section whole.
SQUARES = (1..64).map { |n| n * n }.freeze
NAMES = %w[
  ada alan alonzo barbara claude dennis donald edsger grace guido james
  john ken linus margaret niklaus ole rich robin tony yukihiro
].freeze
CODES = { ok: 0, warn: 1, err: 2, fatal: 3 }.freeze
puts SQUARES.sum
puts NAMES.length, NAMES.max_by(&:length)
p CODES.sort_by { |_, v| -v }.first
__END__
89440
21
margaret
[:fatal, 3]
