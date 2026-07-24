# Class#subclasses returns the direct, defined subclasses from the compile-time
# class graph. (The elements are boxed classes; #to_s works, and the poly `.name`
# on a class element is a separate gap.) Regression toward #2656.
class Base; end
class Kid1 < Base; end
class Kid2 < Base; end
class GKid < Kid1; end
p Base.subclasses.length
p Base.subclasses.map { |c| c.to_s }.sort
p Kid1.subclasses.map { |c| c.to_s }
p Kid2.subclasses
