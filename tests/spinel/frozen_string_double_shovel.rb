s001 = +"x"; s001.freeze
p(begin; s001 << "y"; rescue FrozenError => e001; e001.receiver; end)
p(begin; s001 << "y"; rescue FrozenError => e002; e002.receiver; end)
s2 = +"a"; s2 << "b"; p s2
s3 = "c".dup; s3.freeze; r3 = (begin; s3 << "d"; rescue FrozenError; :fz; end); p r3
p s3.frozen?
