# CRuby distinguishes three states with a Qundef/Qnil/value sentinel
# (rb_f_raise, eval.c:740), and the first two mean OPPOSITE things: an
# omitted `cause:` chains automatically from $!, while `cause: nil`
# SUPPRESSES that chaining. Modeling this as Option<NodeId> would lower
# `cause: nil` to None and silently chain anyway -- so both are asserted
# here, along with the TypeError and circular-cause rejections.

def blow(m); raise "top", cause: ArgumentError.new(m); end
begin
  blow("root")
rescue => e
  p e.cause
  puts e.message
end
begin
  begin
    raise ArgumentError, "inner"
  rescue ArgumentError
    raise "outer"
  end
rescue => e
  p e.cause
end
begin
  begin
    raise ArgumentError, "inner2"
  rescue ArgumentError
    raise "outer2", cause: nil
  end
rescue => e
  p e.cause
end
begin
  raise "x", cause: 5
rescue TypeError => e
  puts "TypeError: #{e.message}"
end
begin
  a = RuntimeError.new("a")
  b = RuntimeError.new("b")
  begin
    raise a, cause: b
  rescue; end
  raise b, cause: a
rescue ArgumentError => e
  puts "ArgumentError: #{e.message}"
end
__END__
#<ArgumentError: root>
top
#<ArgumentError: inner>
nil
TypeError: exception object expected
ArgumentError: circular causes
