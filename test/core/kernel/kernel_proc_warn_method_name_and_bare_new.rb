sq = proc { |x| x * x }
puts sq.call(5)
warn "to stderr"
def who = __method__
p who
GC.start
class F
  def self.create
    new
  end
end
puts F.create.class
__END__
25
:who
F
#@ stderr
to stderr
