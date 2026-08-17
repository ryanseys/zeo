class SubIns < StandardError; end
p(SubIns.new("m").inspect)
i001 = SubIns.new("m").inspect; p i001
p(RuntimeError.new("").inspect)
i002 = RuntimeError.new("").inspect; p i002
p(RuntimeError.new("q").inspect)
p RuntimeError.new.message
p RuntimeError.new("").message
p SubIns.new("").inspect
