# `def m; ...; rescue => e; ...; end` -- `DefNode::body()` is directly a
# `BeginNode` with no `begin_keyword_loc` in this shape (confirmed via
# `Prism.parse`), flowing through the SAME `HirNode::Begin` lowering as
# an explicit `begin`.

class Worker
  def safe
    raise "oops"
  rescue => e
    "handled: #{e.send(:message)}"
  end
end
puts Worker.new.safe
__END__
handled: oops
