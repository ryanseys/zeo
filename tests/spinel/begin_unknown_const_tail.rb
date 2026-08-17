# A begin whose body is an unknown constant: the read emits a raise, whose C
# type is the class struct, and assigning that to the slot the handler's value
# settled on did not compile. Control never reaches the assignment.
p(begin; NoSuchConst1; rescue NameError => e; "x"; end)
p(begin; NoSuchConst2; rescue NameError; 5; end)
p(begin; 7; rescue; 0; end)
p(begin; NoSuchConst3; rescue NameError => e3; e3.class.to_s; end)
def f; NoSuchConst4; rescue NameError; -1; end
p f
p(begin; [1,2]; rescue; 0; end)
