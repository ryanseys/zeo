# frozen_string_literal: true
p "abc".frozen?
p "interp #{1 + 1}".frozen?
p "abc".dup.frozen?
buf = "abc"
begin; buf << "y"; rescue FrozenError => e; puts e.message; end
__END__
true
false
false
can't modify frozen String: "abc"
