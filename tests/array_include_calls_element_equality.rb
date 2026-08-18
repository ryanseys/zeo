class Boom
  def ==(o) = raise("cmp")
end

begin
  p [Boom.new].include?(1)
rescue RuntimeError => e
  p [:raised, e.message]
end

begin
  p [Boom.new].index(1)
rescue RuntimeError => e
  p [:raised, e.message]
end
