# The cycle this program builds on purpose, and the census the
# `ZEO_RT_GCCHECK=1` leg gates against. A cycle alive at exit is not a
# defect: it is a ring the program never broke. What the leg gates is a
# CHANGE to the line below.
# 
# A Deferred holds the proc whose home it is, so the object and its proc own each other.
#@ gccheck: cycle leak: 2 objects (Deferred x1, Proc x1)
# A non-lambda Proc's `return` is a NON-LOCAL return: it returns from the
# method that created the Proc, while that method is still on the stack. Once
# that home method has unwound, the same `return` raises LocalJumpError
# instead of jumping into a freed frame. A lambda's `return` is always local.

# --- Live home: return from the creating method ---
def simple; proc { return 30 }.call; 40; end
p simple                              # 30

def conditional(x); proc { return "yes" if x > 0 }.call; "no"; end
p conditional(5)                      # "yes"
p conditional(-1)                     # "no"

def multi; proc { return 1, 2, 3 }.call; [9]; end
p multi                               # [1, 2, 3]

# Through a builtin iterator (the return unwinds out of `each` and the method).
def first_hit; proc { [1, 2, 3].each { |x| return x * 10 if x == 2 }; :none }.call; end
p first_hit                           # 20

# Through `yield` into a user method.
def gives; yield; end
def via_yield; gives { return 55 }; 66; end
p via_yield                           # 55

# Nested returning-proc methods: inner's return unwinds only inner.
def inner; proc { return "IR" }.call; "IN"; end
def outer; x = inner; proc { return "O:#{x}" }.call; "ON"; end
p outer                               # "O:IR"

# A lambda's return stays local to the lambda.
def with_lambda; -> { return 30 }.call; 40; end
p with_lambda                         # 40

# --- Dead home: LocalJumpError ---
# Home escaped via an exception, Proc called afterward.
$escaped = nil
def home_raises; $escaped = proc { return 99 }; raise "boom"; end
begin; home_raises; rescue; end
begin
  $escaped.call
rescue LocalJumpError => e
  puts "after exception: #{e.message}"
end

# Home returned normally, Proc stored on an object and fired later.
class Deferred
  def arm; @job = proc { return :never }; self; end
  def fire; @job.call; end
end
begin
  Deferred.new.arm.fire
rescue LocalJumpError => e
  puts "after normal exit: #{e.message}"
end
__END__
30
"yes"
"no"
[1, 2, 3]
20
55
"O:IR"
40
after exception: unexpected return
after normal exit: unexpected return
