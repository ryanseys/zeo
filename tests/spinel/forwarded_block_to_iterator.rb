def run001(&b001); [1, 2].map(&b001); end
tri001 = ->(x001) { x001 * 3 }
p run001(&tri001)

def run002(&b002); [1, 2].map(&b002); end
pr002 = proc { |x002| x002 * 3 }; p run002(&pr002)   # Ruby: [3, 6]
def sq003(n003) = n003 * 3
def run003(&b003); [1, 2].map(&b003); end
p run003(&method(:sq003))                            # Ruby: [3, 6]

def run004(&b004); [1, 2].map(&b004); end
p run004 { |x004| x004 * 3 }         # => [3, 6]
def run005(&b005); b005.call(3); end
tri005 = ->(x005) { x005 * 3 }
p run005(&tri005)                    # => 9
p run005(&proc { |x006| x006 * 3 })  # => 9
def sq007(n007) = n007 * n007
p run005(&method(:sq007))            # => 9
def run008; yield 3; end
p run008(&tri005)                    # => 9
