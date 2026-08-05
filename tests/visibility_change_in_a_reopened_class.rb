# Visibility is positional: `private :name` in a REOPENED class body applies
# from that point in program order, and calls made while the method was still
# public succeed. zeo's analyze folds visibility into one order-insensitive
# table (the later `private` is visible to reflection from the start), and
# compiled static call sites never re-check visibility at all -- so the
# explicit-receiver call after the reopen still succeeds where ruby raises
# NoMethodError.
class Toggler
  def pub
    "pub"
  end
end

t = Toggler.new
puts t.pub
puts Toggler.public_method_defined?(:pub).inspect

class Toggler
  private :pub
end

begin
  t.pub
rescue NoMethodError => e
  puts "after reopen: #{e.class}"
end
puts Toggler.public_method_defined?(:pub).inspect
puts Toggler.private_method_defined?(:pub).inspect
