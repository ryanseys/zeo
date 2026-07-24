module PG
  class Error < StandardError; end
  class UndefinedTable < Error; end
end
begin
  raise PG::UndefinedTable, "x"
rescue PG::UndefinedTable => e
  puts(e.is_a?(PG::Error) ? "yes" : "no")          # yes
  puts(e.is_a?(PG::UndefinedTable) ? "yes" : "no") # yes
  puts(e.is_a?(StandardError) ? "yes" : "no")      # yes
  puts(e.is_a?(::StandardError) ? "yes" : "no")    # yes
end
x = PG::UndefinedTable.new
puts(x.is_a?(PG::Error) ? "yes" : "no")            # yes
# is_a? with a namespaced constant-path class argument compares the fully
# qualified exception class name (#3260).
