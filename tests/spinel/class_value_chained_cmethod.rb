# A singleton method that returns the class object, chained into another
# singleton method.
#
# The value was right; only the dispatch was missing. A Class-valued receiver
# carried in a variable already switches on the class id and calls the matching
# class's static method, but a receiver that is itself a CALL was excluded --
# so `Job.set(1).run(2)` was rejected at compile time even though `c = Job;
# c.run(2)` worked. A constant or accessor receiver still resolves statically
# through its own path.
#
# The shape is any builder-style class API: ActiveJob's `set` returns the job
# class so options can be chained before enqueueing.

class Job
  def self.run(x)
    p "run #{x}"
  end

  def self.set(opts)
    self
  end

  def self.by_name(opts)
    Job
  end

  def self.label
    "job"
  end
end

Job.set(1).run(2)
Job.by_name(1).run(3)
p Job.set(1).label
p Job.set(1) == Job

# chained twice
Job.set(1).set(2).run(4)

# two classes defining the same singleton name: the arm is chosen by the
# class the value actually carries
class Other
  def self.run(x)
    p "other #{x}"
  end

  def self.pick(n)
    n == 1 ? Job : Other
  end

  def self.label
    "other"
  end
end

Other.pick(1).run(5)
Other.pick(2).run(6)
p Other.pick(1).label
p Other.pick(2).label

# through a local and an ivar, the shapes that already worked
c = Job.set(1)
c.run(7)

class Holder
  def initialize
    @k = Job.set(1)
  end

  def go
    @k.run(8)
  end
end

Holder.new.go

# a returned class used for instantiation
class Widget
  def initialize(n)
    @n = n
  end

  def n
    @n
  end

  def self.klass
    Widget
  end
end

p Widget.klass.new(9).n
