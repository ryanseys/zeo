# Small built-in method gaps (#1222): __callee__ (resolves like
# __method__ to the current method name), String#clone (like dup), and
# NilClass#itself (returns nil).

def who
  __callee__
end
puts who                  # who

def same?
  __method__ == __callee__
end
puts same?                # true

s = "abc".clone
puts s                    # abc
puts s.length             # 3

m = String.new("mut").clone
puts m                    # mut

p nil.itself              # nil
p nil.itself.nil?         # true
