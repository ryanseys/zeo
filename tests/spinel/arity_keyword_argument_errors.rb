# Statically-detectable argument-shape errors raise ArgumentError at runtime,
# in CRuby's words: too many positionals, an unknown keyword, and a missing
# required keyword. An options-hash call (no keyword matches any parameter)
# still collapses into the positional hash, and splat/rest shapes are exempt.
def m(a, b) = a + b
begin
  puts m(1, 2, 3)
rescue ArgumentError => e
  puts "1: #{e.message}"
end
def k(a:) = a
begin
  puts k(a: 1, zzz: 2)
rescue ArgumentError => e
  puts "2: #{e.message}"
end
class World
  def initialize(width:, height:)
    @w = width
    @h = height
  end
  def area = @w * @h
end
begin
  puts World.new(width: 150).area
rescue ArgumentError => e
  puts "3: #{e.message}"
end
puts World.new(width: 3, height: 4).area
def opts_hash(o = {}) = o.size
puts opts_hash(timeout: 5, retries: 2)
def rest(*xs) = xs.length
puts rest(1, 2, 3, 4)
