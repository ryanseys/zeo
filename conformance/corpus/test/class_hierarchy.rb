# Issue #404 Phase 3 (Tier 1, classes-only). Exercise the
# precomputed ancestors / parents tables added in this phase.
# Modules + dynamic is_a? + case-when on Class come in later
# tiers; this test stays on user-class inheritance.

class A
end

class B < A
end

class C < B
end

# .superclass walks @cls_parents one step.
puts B.superclass.to_s  # => A
puts C.superclass.to_s  # => B
# A's superclass is Object per CRuby.
puts A.superclass == Object ? "A-root-ok" : "A-root-bad"

# .ancestors returns a PolyArray of boxed sp_Class. Iterate and
# concatenate the names so the output exercises sp_box_class +
# the PolyArray iteration over class-tagged poly elements.
def names_of(arr)
  out = ""
  arr.each do |klass|
    out += "," unless out.length == 0
    out += klass.to_s
  end
  out
end

puts names_of(C.ancestors)  # => "C,B,A,Object,Kernel,BasicObject"
puts names_of(A.ancestors)  # => "A,Object,Kernel,BasicObject"

# `<` / `<=` -- proper / non-proper subclass.
puts (C < A) ? "C<A" : "C!<A"
puts (C <= A) ? "C<=A" : "C!<=A"
puts (A < A) ? "A<A" : "A!<A"
puts (A <= A) ? "A<=A" : "A!<=A"
puts (A < C) ? "A<C" : "A!<C"
