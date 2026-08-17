r001 = (begin; begin; raise "x"; rescue 5; :matched; end; rescue TypeError => e001; e001.class.to_s; end); p r001
empty002 = []
r002 = (begin; begin; raise ArgumentError, "a"; rescue *empty002; :matched; end; rescue ArgumentError => e002; e002.message; end); p r002
lst = [ArgumentError, TypeError]
r1 = (begin; raise TypeError, "t"; rescue *lst => e; e.class.to_s; end); p r1
r2 = (begin; raise "x"; rescue *lst; :m; rescue => e; e.class.to_s; end); p r2
r3 = (begin; raise ArgumentError, "a"; rescue *[]; :m; rescue => e; e.message; end); p r3
