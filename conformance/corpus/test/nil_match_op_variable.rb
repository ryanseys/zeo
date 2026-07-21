# NilClass#=~ is always nil and nil !~ re is always true, including when the
# nil is held in a variable (poly receiver path).
n = nil
p(n =~ /x/)
p(n !~ /x/)
m = nil
p(m !~ /abc/)
