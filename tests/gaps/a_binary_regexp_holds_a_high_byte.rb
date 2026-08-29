def t(l); r=(begin; yield.inspect; rescue Exception=>e; "#{e.class}: #{e.message}"; end); puts format("%-24s %s", l, r); end
t("M-a in binary")     { Regexp.new("\\M-a".b) =~ "\xE1".b }
t("M-a binary enc")    { Regexp.new("\\M-a".b).encoding.to_s }
t("M-a binary source") { Regexp.new("\\M-a".b).source.bytes }
t("a raw high byte")   { Regexp.new("\xE1".b) =~ "\xE1".b }
t("raw byte enc")      { Regexp.new("\xE1".b).encoding.to_s }
t("n flag")            { //n.encoding.to_s }
