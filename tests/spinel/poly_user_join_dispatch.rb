# A poly receiver whose class defines `join`: the dedicated poly-join arm
# answered the receiver's #to_s and never entered the method, because it did not
# ask whether a user class owned the name the way its neighbours do (#4071). The
# call has to dispatch, and an Array reaching the same call still has to join.
class Row
  def initialize = @ran = false
  attr_reader :ran

  def join(sep)
    @ran = true
    "row" + sep
  end
end

def joined(v) = v.join("-")

r = Row.new
p joined(r)
p r.ran
p joined(["x", "y"])
p joined([1, 2])

# the zero-argument form reaches the other dispatch, whose array arms had to be
# added alongside the user ones
class Glue
  def join = "glue"
end

def j0(v) = v.join

p j0(Glue.new)
p j0(["a", "b"])
p j0([1, 2])
