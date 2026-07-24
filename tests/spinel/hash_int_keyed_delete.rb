# Integer-keyed Hash delete-family + Hash#shift (KieranP #2344,#2349,#2353)
p({ 1 => "a", 2 => "b" }.delete(1))         # #2344 delete
p({ 1 => 10, 2 => 20 }.delete(2))
p({ 1 => 10, 2 => 20 }.slice(1))            # #2344 slice
p({ 1 => 10, 2 => 20 }.except(1))           # #2344 except
p({ 1 => "a", 2 => "b" }.slice(2))
h = { a: 1, b: 2 }
p h.shift                                    # #2349 shift
p h
p({ 1 => 10, 2 => 20 }.shift)
p({}.shift) rescue p(:empty_ok)
h2 = { 1 => 10, 2 => 20 }
h2.delete_if { |_k, v| v > 15 }              # #2353 delete-family mutator
p h2
p({ 1 => 10 }.value?(10))                    # #2353 value?
p({ 1 => 10 }.has_value?(99))
