class A; def id; 1; end; end
class B; def id; 2; end; end
def txn; r = yield; r; end
def ba; txn { t = A.new; t }; end
def bb; txn { t = B.new; t }; end
x = ba; y = bb; puts x.id; puts y.id

class C; def id; 3; end; end
def one; txn { u = C.new; u }; end
puts one.id
