# The value of a spliced block is its tail, and a tail CALL whose statement
# form drops that value has to be emitted as an expression: tap answers its
# receiver, not its block's body, and a receiver-returning iterator on a
# computed receiver answers the receiver (#4155).
class Rec
  def hello = "hi"
end

module Conn
  def self.transaction
    yield
  end
end

p Conn.transaction { Rec.new.tap { |r| r.hello } }.hello
p Conn.transaction { Rec.new.tap { |r| r } }.hello
p Conn.transaction { Rec.new }.hello
p Conn.transaction { 5.then { |n| n * 2 } }
p Conn.transaction { [3, 1, 2].each { |x| x } }
p Conn.transaction { [1, 2].map { |x| x + 1 } }
p Conn.transaction { [3, 1, 2].dup.each { |x| x } }
