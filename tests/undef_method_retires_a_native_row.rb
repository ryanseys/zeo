# `undef_method` writes a TOMBSTONE in the overlay -- the only record that a
# native row was retired, since a builtin's `class_table` is a static list.
# `respond_to?`/`method_defined?` already read it; dispatch did not, so the
# predicate said no and the call still answered.

class Array
  undef_method :zip
end
p Array.method_defined?(:zip)
p [1].respond_to?(:zip)
begin; [1].zip([2]); rescue NoMethodError => e; p e.class; end
begin; [1].send(:zip, [2]); rescue NoMethodError => e; p [:send, e.class]; end
# Everything else on the class is untouched.
p [3, 1, 2].sort
p [1, 2].map { |x| x * 2 }

# A SUBCLASS retires the inherited row for itself only.
class Stack < Array
  undef_method :size
end
p Stack.method_defined?(:size)
begin; Stack.new.size; rescue NoMethodError => e; p e.class; end
p Stack.new.length
p [1, 2].size

# The bare `undef` keyword reaches the same rows.
class Queue2 < Array
  undef push
end
begin; Queue2.new.push(1); rescue NoMethodError => e; p e.class; end
p Queue2.new << 1

# A name no ancestor defines is a NameError, not a silent no-op.
begin
  Array.send(:undef_method, :no_such_method_at_all)
rescue NameError => e
  p e.class
end

# A user class is unaffected by the builtin tombstones.
class Plain2
  def size; :plain; end
end
p Plain2.new.size
