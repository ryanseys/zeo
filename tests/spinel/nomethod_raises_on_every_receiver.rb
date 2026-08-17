# An undefined method raises a rescuable NoMethodError wherever CRuby does:
# an Array/Hash/Symbol literal and a user-class instance were refused at
# compile time, so a program that only reaches the call behind a rescue could
# not be built at all (#3811).
p(begin; [1].nope; rescue NoMethodError => e; e.receiver; end)
p(begin; :s.nope; rescue NoMethodError => e; e.message; end)
p(begin; ({ a: 1 }).nope; rescue NoMethodError => e; e.name; end)
class Plain; end
p(begin; Plain.new.nope; rescue NoMethodError => e; e.message; end)
xs = [1, 2]
p(begin; xs.nope; rescue NoMethodError => e; e.message; end)
p(begin; (1..3).nope; rescue NoMethodError => e; e.message; end)
p(begin; nil.nope; rescue NoMethodError => e; e.message; end)
p(begin; "s".nope; rescue NoMethodError => e; e.message; end)
