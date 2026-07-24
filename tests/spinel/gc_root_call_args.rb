# Regression for #1057: a call's argument temporaries must stay GC-
# rooted across evaluation of its sibling arguments. Each case puts a
# fresh heap string in the slot gcc evaluates first (the last one)
# beside a sibling whose evaluation triggers a collection. Without
# rooting, the fresh string is freed mid-call and the callee reads
# reused/garbage memory.

def churn_gc(x)
  GC.start
  i = 0
  s = ""
  while i < 8000
    s = s + i.to_s + ","
    i += 1
  end
  x
end

def join2(a, b)
  a + "|" + b
end

class Joiner
  def join2(a, b)
    a + "|" + b
  end
end

# top-level method call
puts join2(churn_gc("A"), "B_" + 1.to_s)
# instance method call
puts Joiner.new.join2(churn_gc("C"), "D_" + 2.to_s)
# sprintf / string interpolation
label = "E"
puts "[#{churn_gc("F")}|#{label + "_" + 3.to_s}]"
