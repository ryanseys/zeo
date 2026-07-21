# An empty method body implicitly returns nil.
def f; end
puts f.nil?
def g
end
x = g
puts x.nil?
puts f.inspect
