# Hash#to_h is the identity, so a BOXED receiver keeps the variant it really
# holds. Narrowing it to the symbol-keyed one raised on a String-keyed hash
# reached through a defaulted parameter (#3972), and an array of pairs whose
# keys are not Symbols was read as symbol ids.
def tag(name, opts = {})
  merged = { "name" => name }.merge(opts.to_h)
  merged.map { |k, v| "#{k}=#{v}" }.join(" ")
end
p tag("a")
p tag("b", { "x" => "1" })
p tag("c", { y: 2 })

def norm(v)
  v.to_h
end
p norm({ "s" => 1 })
p norm({ s: 1 })
p norm(nil)
p norm([[1, 2]])
p({ "a" => 1 }.to_h)
p({ a: 1 }.to_h)
