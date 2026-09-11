# Under /i an ASCII-range member of a bracket class (`\w`, `\W`, `(?a)` POSIX)
# folds only within ASCII, so `ſ` and the Kelvin sign never arrive through `s`
# and `k`. Ranges, literals, `\d`, `\s`, `\h` and `\p` still fold past ASCII,
# and an intersection folds each side of ASCII on its own.
rows = %w[
  [\w] [\W] [^\w] [^\W] [\w&&[a-z]] [a-z\w] [\d] [\s] [\h] [\p{Word}]
  [[:word:]] [[:alpha:]] [[:upper:]] [[:lower:]] (?a)[[:word:]]
  (?a)[[:alpha:]] (?a)[[:upper:]] (?a)[[:lower:]] (?a)[\w] (?u)[\w]
  [k\w] [s\w] [\x00-\x7f] [s] [k] [^a-z] [\w\d] [[\w]] [^[\w]] [\wé]
  [\w&&[ſ]] [^\w&&[a-z]] [a-z&&\w] [\W&&[^ſ]]
]
subjects = ["ſ", "K", "k", "S", "é", "É", "a"]
puts "pattern".ljust(18) + %w[ſ KE k S é É a].map { _1.ljust(3) }.join
rows.each do |r|
  re = Regexp.new(r, Regexp::IGNORECASE)
  puts r.ljust(18) + subjects.map { |s| (re =~ s ? "y" : ".").ljust(3) }.join
end
p "Straße ſ K".scan(/[\w]+/i)
p "(?i:[\\w])".then { Regexp.new(_1) }.match?("ſ")
__END__
pattern           ſ  KE k  S  é  É  a  
[\w]              .  .  y  y  .  .  y  
[\W]              y  y  .  .  y  y  .  
[^\w]             y  y  .  .  y  y  .  
[^\W]             .  .  y  y  .  .  y  
[\w&&[a-z]]       .  .  y  y  .  .  y  
[a-z\w]           y  y  y  y  .  .  y  
[\d]              .  .  .  .  .  .  .  
[\s]              .  .  .  .  .  .  .  
[\h]              .  .  .  .  .  .  y  
[\p{Word}]        y  y  y  y  y  y  y  
[[:word:]]        y  y  y  y  y  y  y  
[[:alpha:]]       y  y  y  y  y  y  y  
[[:upper:]]       y  y  y  y  y  y  y  
[[:lower:]]       y  y  y  y  y  y  y  
(?a)[[:word:]]    .  .  y  y  .  .  y  
(?a)[[:alpha:]]   .  .  y  y  .  .  y  
(?a)[[:upper:]]   .  .  y  y  .  .  y  
(?a)[[:lower:]]   .  .  y  y  .  .  y  
(?a)[\w]          .  .  y  y  .  .  y  
(?u)[\w]          y  y  y  y  y  y  y  
[k\w]             .  y  y  y  .  .  y  
[s\w]             y  .  y  y  .  .  y  
[\x00-\x7f]       y  y  y  y  .  .  y  
[s]               y  .  .  y  .  .  .  
[k]               .  y  y  .  .  .  .  
[^a-z]            .  .  .  .  y  y  .  
[\w\d]            .  .  y  y  .  .  y  
[[\w]]            .  .  y  y  .  .  y  
[^[\w]]           y  y  .  .  y  y  .  
[\wé]             .  .  y  y  y  y  y  
[\w&&[ſ]]         .  .  .  .  .  .  .  
[^\w&&[a-z]]      y  y  .  .  y  y  .  
[a-z&&\w]         .  .  y  y  .  .  y  
[\W&&[^ſ]]        .  y  .  .  y  y  .  
["Stra", "e", "K"]
false
