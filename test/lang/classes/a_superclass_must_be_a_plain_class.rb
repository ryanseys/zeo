# Superclass validation: a Module raises TypeError "superclass must be an
# instance of Class (given an instance of Module)" (zeo: NoMethodError on
# 'inherited'); a non-class names the given class the same way (zeo's
# message is the older "must be a Class"); and `Class.new(Class)` is
# refused outright ("can't make subclass of Class") where zeo mints one.
# (Found by the 2026-08-24 probe sweep.)
def show
  yield
rescue Exception => e
  puts "#{e.class}: #{e.message}"
end
show { Class.new(Comparable) }
show { Class.new(1) }
show { Class.new(Class) && puts("subclass of Class minted") }
__END__
TypeError: superclass must be an instance of Class (given an instance of Module)
TypeError: superclass must be an instance of Class (given an instance of Integer)
TypeError: can't make subclass of Class
