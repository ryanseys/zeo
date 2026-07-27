class A; def id; 1; end; end
class B; def id; 2; end; end
def txn; result = yield; result; end
def ba; txn { A.new }; end
def bb; txn { B.new }; end
x = ba; y = bb; puts x.id; puts y.id
