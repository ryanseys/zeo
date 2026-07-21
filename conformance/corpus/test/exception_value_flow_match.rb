# Exception values flowing through begin/rescue expressions, case/when and
# Module#=== matching, and runtime-recovered NameError#name.
e = (begin; raise "z"; rescue => x; x; end)
p e.message
c = (begin; begin; raise "inner"; rescue; raise "outer"; end; rescue => e2; e2.cause; end)
p c.message
def classify(ex)
  case ex
  when KeyError then "key"
  when IndexError then "index"
  when StandardError then "std"
  else "other"
  end
end
begin; raise KeyError, "k"; rescue => r1; p classify(r1); end
begin; raise StopIteration; rescue => r2; p classify(r2); end
begin; raise "r"; rescue => r3; p classify(r3); end
begin
  raise ArgumentError, "a"
rescue => ex
  p StandardError === ex
  p TypeError === ex
  k = StandardError
  p k === ex
end
begin; nil.foo(1); rescue NoMethodError => n1; p n1.name; end
n2 = NoMethodError.new("m", :bar)
p n2.name
begin; raise NameError, "custom message"; rescue NameError => n3; p n3.name; end
