def t(l); r=(begin; yield.inspect; rescue Exception=>e; "#{e.class}: #{e.message}"; end); puts format("%-22s %s", l, r); end
t("Regexp.allocate")   { Regexp.allocate.class }
t("Regexp source")     { Regexp.allocate.source }
t("Mutex.allocate")    { Mutex.allocate.class }
t("Queue.allocate")    { Queue.allocate.class }
t("Proc.allocate")     { Proc.allocate.class }
t("MatchData.allocate"){ MatchData.allocate.class }
t("String.allocate")   { String.allocate }
t("Array.allocate")    { Array.allocate }
t("Hash.allocate")     { Hash.allocate }
t("Time.allocate")     { Time.allocate.class }
