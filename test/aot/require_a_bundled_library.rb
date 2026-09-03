# A whole pure-Ruby require graph is spliced into the binary and runs from it.
require "csv"
require "set"
rows = CSV.parse("name,lang\nzeo,rust\nze0,ruby\n", headers: true)
rows.each { |r| puts "#{r['name']} -> #{r['lang']}" }
langs = Set.new(rows.map { |r| r["lang"] })
p langs.include?("rust"), langs.size
__END__
zeo -> rust
ze0 -> ruby
true
2
