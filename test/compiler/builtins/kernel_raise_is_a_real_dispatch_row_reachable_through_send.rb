# `obj.send(:raise, ...)` used to be NoMethodError (raise existed only as
# a parse-time lowering). Now a real Kernel row mirrors CRuby's
# `rb_make_exception`: class+message, bare re-raise from `$!`, a bare
# class defaulting its message to the class name, a String implying
# RuntimeError, and the non-exception TypeError. All oracle-verified.

o = Object.new
begin
  o.send(:raise, ArgumentError, "via send")
rescue ArgumentError => e
  puts e.message
end
begin
  o.send(:raise, ArgumentError)
rescue ArgumentError => e
  puts e.message
end
begin
  o.send(:raise, "bare msg")
rescue RuntimeError => e
  puts e.message
end
begin
  o.send(:raise)
rescue RuntimeError => e
  puts "[#{e.message}]"
end
begin
  begin
    raise IOError, "orig"
  rescue
    o.send(:raise)
  end
rescue IOError => e
  puts e.message
end
begin
  o.send(:raise, 42)
rescue TypeError => e
  puts e.message
end
__END__
via send
ArgumentError
bare msg
[]
orig
exception class/object expected
