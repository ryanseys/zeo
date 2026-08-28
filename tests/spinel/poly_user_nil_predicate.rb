# A Null Object answers `nil?` with true, and its guard has to fire. The
# poly-receiver arm folded `nil?` to the tag test without asking whether a user
# class owns the name -- the arm directly below it skips for exactly that
# reason, and this one did not. The user's method was never entered and
# `return ... if u.nil?` silently did not fire.
class NullUser
  def nil? = true
  def name = "(none)"
end

class RealUser
  def nil? = false
  def name = "matz"
end

def label(u)
  return "missing" if u.nil?
  u.name
end

p label(NullUser.new)
p label(RealUser.new)
p label(nil)

def ask(v) = v.nil?

p ask(NullUser.new)
p ask(RealUser.new)
p ask(nil)

# a union with no real nil in it, which is where the fold was reached
class Only
  def nil? = true
end

def ask2(v) = v.nil?
p ask2(Only.new)
p ask2([1])
p ask2("s")

# and with no user class defining it, the fold is what it always was
def plain(v) = v.nil?
p plain(nil)
p plain(1)
p plain("a")
p plain([1])
