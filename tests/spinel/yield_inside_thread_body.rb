# yield inside a Thread body sees the enclosing method's block
def run
  t = Thread.new { yield 5 }
  t.value
end
p(run { |x| x * 2 })

def run2
  t = Thread.new do
    r = nil
    loop { r = yield(5); break }
    r
  end
  t.value
end
p(run2 { |x| x * 2 })

def run3
  ts = 2.times.map { |i| Thread.new { yield i } }
  ts.map(&:value)
end
p(run3 { |x| x * 10 })

def run4(&blk)
  t = Thread.new { blk.call(7) }
  t.value
end
p(run4 { |x| x + 1 })

def plain
  yield 3
end
p(plain { |x| x * 4 })
