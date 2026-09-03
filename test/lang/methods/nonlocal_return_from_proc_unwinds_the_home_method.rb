# A non-lambda Proc's `return` returns from its creating method (live
# home), including through builtin iterators and `yield`; a lambda's is
# local.

def simple; proc { return 30 }.call; 40; end
p simple
def cond(x); proc { return "yes" if x > 0 }.call; "no"; end
p cond(5); p cond(-1)
def multi; proc { return 1, 2, 3 }.call; [9]; end
p multi
def thru_each; proc { [1,2,3].each { |x| return x*10 if x==2 }; :none }.call; end
p thru_each
def thru_ewi; proc { [10,20,30].each_with_index { |v,i| return i if v==20 }; -1 }.call; end
p thru_ewi
def gives; yield; end
def via_yield; gives { return 55 }; 66; end
p via_yield
def inner; proc { return "IR" }.call; "IN"; end
def outer; x = inner; proc { return "O:#{x}" }.call; "ON"; end
p outer
def cd(n, acc); proc { return acc if n==0 }.call; cd(n-1, acc+n); end
p cd(5, 0)
def with_lambda; -> { return 30 }.call; 40; end
p with_lambda
def dbl(x); proc { return x*2 }.call; -1; end
s = 0; 300.times { |i| s += dbl(i) }; p s
__END__
30
"yes"
"no"
[1, 2, 3]
20
1
55
"O:IR"
15
40
89700
