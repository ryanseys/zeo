def run001(&b001); b001.call(3); end
tri001 = ->(x) { x * 3 }
p run001(&tri001)

def run002(&b002); b002.call(3); end
pr002 = proc { |x| x * 3 }
p run002(&pr002)

def sq003(n) = n * n
def run003(&b003); b003.call(3); end
p run003(&method(:sq003))

def run004(&b004); b004.call(3); end
p run004(&->(x) { x * 3 })

def run005; yield 3; end
tri005 = ->(x) { x * 3 }
p run005(&tri005)

def twice(&b); [b.call(1), b.call(2)]; end
p twice(&->(x) { x + 10 })

def each_of(a, &b); a.each { |x| b.call(x) }; end
out = []
each_of([1, 2, 3], &->(x) { out << x * 2 })
p out
