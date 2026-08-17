class SubIns < StandardError; end
p(SubIns.new("m").inspect)
i = SubIns.new("m").inspect
p i
p(RuntimeError.new("m").inspect)
p(StandardError.new.inspect)
p(SubIns.new.inspect)
begin
  raise SubIns, "z"
rescue => e
  p e.inspect
end
p [RuntimeError.new("boxed")].first.inspect
