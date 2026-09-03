# frozen_string_literal: true
# Frozen literals and symbols live in the binary's data section and keep
# their identity across uses.
A = "shared literal"
B = "shared literal"
puts A.frozen?, A.equal?(B)
SYMS = %i[alpha beta gamma].freeze
p SYMS
p SYMS.map(&:to_s).map(&:frozen?)
p :alpha.equal?("alpha".to_sym)
__END__
true
true
[:alpha, :beta, :gamma]
[false, false, false]
true
