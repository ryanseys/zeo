def memo_get(n001, memo001 = {})
  memo001[n001] = n001 * 2
  memo001[n001]
end
p memo_get(3)

def probe(n002, memo002 = {})
  memo002[n002] = n002 * 2
  p memo002.key?(n002)   # Ruby: true   Spinel: false
  p memo002.size         # both: 1
  memo002[n002]
end
p probe(3)

def depth(name003, edges003, memo003 = {})
  return memo003[name003] if memo003.key?(name003)
  d003 = edges003[name003] || []
  memo003[name003] = d003.empty? ? 0 : 1 + d003.map { |x003| depth(x003, edges003, memo003) }.max
  memo003[name003]
end
p depth("a", { "a" => ["b"], "b" => ["c"], "c" => [] })

def arr_val(s004, memo004 = {})
  memo004[s004] = [1]
  memo004[s004]
end
p arr_val("s")   # Ruby: [1]

def with_arg(n005, memo005)
  memo005[n005] = n005 * 2
  memo005[n005]
end
p with_arg(3, {})     # => 6

def sym_key(s006, memo006 = {})
  memo006[s006] = 1
  memo006[s006]
end
p sym_key(:a)         # => 1

def acc_push(n007, acc007 = [])
  acc007 << n007
  acc007.size
end
p acc_push(3)         # => 1
