# A class-body guard folds with the same superset the top level uses, in the
# body's own cref. Before the lift this whole shape was a compile error ("a
# position the analyze walk doesn't register"). The probe names a class
# registered BEFORE this body began -- a probe of a sibling statement the
# walk has not reached stays undecidable rather than misfolding.
class Helper; end

module Outer
  if defined?(Helper)
    module Inner
      WHO = "folded"
    end
  end
end
puts Outer::Inner::WHO
__END__
folded
