begin
  nil.nope
rescue NoMethodError => e
  loc = e.backtrace_locations.first
  p RubyVM::AbstractSyntaxTree.node_id_for_backtrace_location(loc)
end
