r001 = (begin; ({a: 1}).nope; rescue NoMethodError => e001; e001.receiver; end); p r001

p(begin; ({}).nope; rescue NoMethodError => e; e.receiver; end)
# Ruby: {}   Spinel: nil
p(begin; ({"x" => 1}).nope; rescue NoMethodError => e; e.receiver; end)
# Ruby: {"x" => 1}   Spinel: nil
p(begin; ({a: 1}).nope; rescue NoMethodError => e; e.receiver.class; end)
# Ruby: Hash   Spinel: NilClass

h = {a: 1}
p(begin; h.nope; rescue NoMethodError => e; e.receiver; end)      # => {a: 1}
p(begin; ({a: 1}).nope; rescue NoMethodError => e; e.name; end)   # => :nope
p(begin; ({a: 1}).nope; rescue NoMethodError => e; e.message; end)
#   => "undefined method 'nope' for an instance of Hash"
p(begin; [1].nope; rescue NoMethodError => e; e.receiver; end)    # => [1]
p(begin; :s.nope; rescue NoMethodError => e; e.receiver; end)     # => :s
