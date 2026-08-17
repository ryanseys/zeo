# A modifier rescue catches StandardError and its descendants only, the way the
# full begin/rescue form does. This arm caught everything, so a subclass of
# Exception was swallowed.
class NotStd < Exception; end
class Std < StandardError; end

p((raise(Std, "s") rescue "caught-std"))
p((raise("plain") rescue "caught-runtime"))
p((raise(ArgumentError, "a") rescue "caught-arg"))

# a non-StandardError passes straight through the modifier rescue
r = begin
  (raise(NotStd, "x") rescue "should-not-catch")
rescue Exception => e
  [e.class.to_s, e.message]
end
p r

# and an explicit rescue of it still catches
s = begin
  raise NotStd, "y"
rescue NotStd => e2
  e2.message
end
p s
