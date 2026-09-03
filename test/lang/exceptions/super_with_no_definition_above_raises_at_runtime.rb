# Real Ruby has no definition-time check for `super`: it resolves against
# the receiver's live ancestry at CALL time and raises NoMethodError only
# if that walk comes up empty (vm_search_super_method / vm_eval.c). So a
# `super` with nothing above it must compile, and the raise must be
# rescuable -- rejecting it at compile time would kill this whole program.

class Rec
  def as_json
    h = super
    h[:x] = 1
    h
  end
end
begin
  Rec.new.as_json
rescue NoMethodError => e
  puts e.message
end
__END__
super: no superclass method 'as_json' for an instance of Rec
