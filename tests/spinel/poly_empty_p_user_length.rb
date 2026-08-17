# Asking whether a user class owns the CALLED name: a class defining `length`
# used to send every `empty?` on a boxed receiver into an unconditional
# NoMethodError, whatever the receiver turned out to be (#3805).
class Sized
  def length
    0
  end
end

class Baggy
  def empty?
    true
  end
end

opts = { n: 1, list: [2], s: 'ab', e: [] }
p opts[:list].empty?
p opts[:e].empty?
p opts[:s].empty?
p [2].empty?

# the guard #1438 added still holds: a user class owning the name wins
things = [Baggy.new, [1]]
p things.first.empty?

# and a user object with no #empty? raises rather than answering true
sized = [Sized.new, 1]
begin
  p sized.first.empty?
rescue NoMethodError => e
  puts "NoMethodError"
end
